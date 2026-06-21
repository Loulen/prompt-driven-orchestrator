//! Admission control for the global concurrent-NodeRun-session cap.
//!
//! PDO bounds the number of live NodeRun (Claude Code) tmux sessions
//! running at once — the resource that actually collapses under load (the
//! tmux-server collapse from closed #78). The cap is enforced *per node-session
//! spawn*, not per Run: a Run is admitted immediately, but each of its nodes
//! must win an admission slot before its session is spawned. A node that cannot
//! get a slot enters the `waiting` state and is spawned once a slot frees.
//!
//! Pipeline Manager sessions are deliberately *not* counted (they are light,
//! one per Run, and counting them risks a soft-deadlock where N managers
//! saturate the budget with no slot left for real work).
//!
//! This module is pure: it makes the decision and counts live sessions from
//! projected run state. The dispatcher owns the side effects (spawning,
//! emitting the `waiting` event).

use crate::event_log::{NodeStatus, RunState};

/// Env var that overrides the global session cap. Default: [`DEFAULT_SESSION_CAP`].
///
/// The instance-wide settings page that will own this value is #129 (out of
/// scope here); v1 reads it from a default constant or this env var.
pub const SESSION_CAP_ENV: &str = "PDO_SESSION_CAP";

/// Default global cap on concurrent NodeRun sessions.
///
/// Kept below the ~30-session point where the tmux server was observed to
/// collapse (#77/#78), leaving headroom for the per-Run manager sessions that
/// are exempt from the cap. 20 trades more parallelism for a slimmer margin —
/// on a memory-constrained box, lower it via `PDO_SESSION_CAP`.
pub const DEFAULT_SESSION_CAP: usize = 20;

/// Whether a new NodeRun session may be admitted given the current count of
/// live sessions and the configured cap.
///
/// Mirrors the spec's `live_sessions + 1 > cap` back-pressure rule: admit only
/// while spawning one more session stays within the cap (equivalently, while a
/// free slot remains).
pub fn can_admit(live_sessions: usize, cap: usize) -> bool {
    live_sessions < cap
}

/// The configured global session cap.
///
/// Reads [`SESSION_CAP_ENV`] if set to a positive integer, else falls back to
/// [`DEFAULT_SESSION_CAP`]. A zero or unparseable value is ignored (a cap of 0
/// would deadlock every Run), so the default applies.
pub fn configured_cap() -> usize {
    std::env::var(SESSION_CAP_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_SESSION_CAP)
}

/// Count the live NodeRun sessions across all known runs.
///
/// Only nodes belonging to a *live* Run ([`RunStatus::is_live`]:
/// `Running`/`AwaitingUser`/`Paused`) are counted. A terminal Run
/// (`Completed`/`Failed`/`Halted`/`Archived`) spawns no new work, so a node it
/// still projects as session-holding is a projection artifact — its tmux
/// session has been (or is about to be) reaped — and must not consume an
/// admission slot. Counting such phantoms permanently leaked a slot from the
/// global cap (#215).
///
/// Within a live Run, a NodeRun session is "live" while its node is `Running`
/// or `AwaitingUser` (an interactive node keeps its tmux session attachable
/// indefinitely). Nodes that are `Pending`, `Waiting`, `Completed`, `Failed`,
/// `Stopped` or `Stale` hold no session and do not count.
///
/// Pipeline Manager sessions are not represented as nodes in the run state, so
/// they are excluded by construction.
pub fn count_live_node_sessions<'a>(runs: impl IntoIterator<Item = &'a RunState>) -> usize {
    runs.into_iter()
        .filter(|run| run.status.is_live())
        .flat_map(|run| run.nodes.values())
        .filter(|node| node_holds_session(&node.status))
        .count()
}

/// Whether a node in the given status is currently holding a NodeRun tmux
/// session (and therefore consuming an admission slot).
///
/// `pub(crate)` so boot recovery can reuse the canonical "session-holding"
/// definition when reconciling dangling nodes of terminal runs (#215).
pub(crate) fn node_holds_session(status: &NodeStatus) -> bool {
    matches!(status, NodeStatus::Running | NodeStatus::AwaitingUser)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{NodeState, RunStatus};

    fn run_with_nodes(run_id: &str, statuses: &[(&str, NodeStatus)]) -> RunState {
        let mut run = RunState::new(run_id.into(), "test".into());
        for (id, status) in statuses {
            run.nodes.insert(
                (*id).into(),
                NodeState {
                    node_id: (*id).into(),
                    status: status.clone(),
                    iter: 1,
                    started_at: None,
                    completed_at: None,
                    failure_reason: None,
                    iterations: Vec::new(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                },
            );
        }
        run
    }

    #[test]
    fn counts_only_running_and_awaiting_nodes_as_live_sessions() {
        let run = run_with_nodes(
            "r1",
            &[
                ("a", NodeStatus::Running),
                ("b", NodeStatus::AwaitingUser),
                ("c", NodeStatus::Pending),
                ("d", NodeStatus::Completed),
                ("e", NodeStatus::Failed),
            ],
        );
        assert_eq!(count_live_node_sessions([&run]), 2);
    }

    #[test]
    fn sums_live_sessions_across_multiple_runs() {
        let r1 = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        let r2 = run_with_nodes(
            "r2",
            &[("b", NodeStatus::Running), ("c", NodeStatus::AwaitingUser)],
        );
        assert_eq!(count_live_node_sessions([&r1, &r2]), 3);
    }

    #[test]
    fn excludes_archived_runs_from_the_count() {
        let mut archived = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        archived.status = RunStatus::Archived;
        let live = run_with_nodes("r2", &[("b", NodeStatus::Running)]);
        assert_eq!(count_live_node_sessions([&archived, &live]), 1);
    }

    #[test]
    fn excludes_failed_run_with_a_running_node() {
        // #215: a run fails (fail-fast) but a sibling node is still projected
        // Running for a window. Its phantom session must not leak a slot.
        let mut failed = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        failed.status = RunStatus::Failed;
        assert_eq!(count_live_node_sessions([&failed]), 0);
    }

    #[test]
    fn excludes_completed_run_with_an_awaiting_user_node() {
        // #215: an interactive node left AwaitingUser inside a Completed run is
        // a projection artifact, not a live session.
        let mut completed = run_with_nodes("r1", &[("a", NodeStatus::AwaitingUser)]);
        completed.status = RunStatus::Completed;
        assert_eq!(count_live_node_sessions([&completed]), 0);
    }

    #[test]
    fn excludes_halted_run_with_a_running_node() {
        // #215: Halted is terminal-but-resumable; while halted it holds no live
        // session, so its nodes do not count.
        let mut halted = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        halted.status = RunStatus::Halted;
        assert_eq!(count_live_node_sessions([&halted]), 0);
    }

    #[test]
    fn excludes_skipped_run_with_a_running_node() {
        // #245: a graceful no-op (Skipped) is terminal; a node still projected
        // Running inside it is a phantom and must not consume an admission slot.
        let mut skipped = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        skipped.status = RunStatus::Skipped;
        assert_eq!(count_live_node_sessions([&skipped]), 0);
    }

    #[test]
    fn counts_a_running_node_in_a_paused_run() {
        // Regression guard: Paused is *live*, not terminal. Don't over-exclude
        // it — a paused run's Running node still holds its session and slot.
        let mut paused = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        paused.status = RunStatus::Paused;
        assert_eq!(count_live_node_sessions([&paused]), 1);
    }

    #[test]
    fn a_waiting_node_holds_no_session() {
        // A node throttled into `waiting` has not spawned a tmux session yet,
        // so it must not consume an admission slot.
        let run = run_with_nodes(
            "r1",
            &[("a", NodeStatus::Running), ("b", NodeStatus::Waiting)],
        );
        assert_eq!(count_live_node_sessions([&run]), 1);
    }

    #[test]
    fn configured_cap_reads_env_then_falls_back_to_default() {
        let saved = std::env::var(SESSION_CAP_ENV).ok();

        std::env::remove_var(SESSION_CAP_ENV);
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);

        std::env::set_var(SESSION_CAP_ENV, "3");
        assert_eq!(configured_cap(), 3);

        // Garbage and zero are ignored (a 0 cap would deadlock every Run).
        std::env::set_var(SESSION_CAP_ENV, "not-a-number");
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);
        std::env::set_var(SESSION_CAP_ENV, "0");
        assert_eq!(configured_cap(), DEFAULT_SESSION_CAP);

        match saved {
            Some(v) => std::env::set_var(SESSION_CAP_ENV, v),
            None => std::env::remove_var(SESSION_CAP_ENV),
        }
    }

    #[test]
    fn admits_while_a_free_slot_remains() {
        // 7 live, cap 10 -> the 8th session fits.
        assert!(can_admit(7, 10));
    }

    #[test]
    fn rejects_once_the_cap_is_reached() {
        // 10 live, cap 10 -> the 11th would exceed the cap.
        assert!(!can_admit(10, 10));
    }

    #[test]
    fn admits_the_session_that_fills_the_last_slot() {
        // 9 live, cap 10 -> the 10th session exactly fills the cap.
        assert!(can_admit(9, 10));
    }
}
