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

/// The configured global session cap, resolving `stored → env → default`
/// (#129, ADR-0015).
///
/// `stored` is the instance-wide setting persisted via the settings page (or
/// `None` when unset). A stored value `>= 1` wins; otherwise the env var
/// [`SESSION_CAP_ENV`] (if a positive integer) applies; otherwise
/// [`DEFAULT_SESSION_CAP`]. A zero/negative stored or env value is ignored — a
/// cap of 0 would deadlock every Run (`can_admit` = `live < cap`).
///
/// The module stays pure: the caller loads the stored value (from
/// `instance_config`) and passes it in. [`configured_cap`] is the
/// `stored = None` shorthand, preserving the env-only behaviour every existing
/// test relies on.
pub fn configured_cap_with(stored: Option<usize>) -> usize {
    stored
        .filter(|&n| n >= 1)
        .or_else(env_cap)
        .unwrap_or(DEFAULT_SESSION_CAP)
}

/// The configured global session cap from the env var alone (`stored = None`).
///
/// Retained so existing call sites and tests that never touch the store keep
/// their exact behaviour.
pub fn configured_cap() -> usize {
    configured_cap_with(None)
}

/// The session cap contributed by [`SESSION_CAP_ENV`] alone (ignoring any stored
/// value), or `None` when unset, unparseable, or zero (a 0 cap would deadlock).
///
/// Exposed so `GET /settings` can disclose a shadowed env var and compute the
/// winning tier identically to [`configured_cap_with`] (#129, ADR-0015).
pub fn env_cap() -> Option<usize> {
    std::env::var(SESSION_CAP_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
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
    count_live_node_sessions_excluding(runs, None)
}

/// The one node-session slot a spawn is **taking back**, and must therefore not be
/// counted against itself (#489-C).
///
/// The key is the full triple. `(node_id, iter)` alone is an over-admission bug:
/// the count is global across Runs while node ids are local to a pipeline, so two
/// concurrent Runs of the same pipeline both carry an `implementer` at `iter 1` —
/// and a Run-blind exclusion would discount the *other* Run's live session, letting
/// the cap be exceeded. That is the very collapse this module exists to prevent.
///
/// `iter` is an `i64`, as everywhere else in the event log.
pub struct SlotExclusion<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
}

/// [`count_live_node_sessions`], minus at most one slot (#489-C).
///
/// The exclusion applies only when that exact `(run_id, node_id, iter)` is
/// **currently session-holding**. Without that condition it would be a free `+1`
/// on any spawn of a non-live iteration, which would raise the effective cap.
pub fn count_live_node_sessions_excluding<'a>(
    runs: impl IntoIterator<Item = &'a RunState>,
    exclude: Option<SlotExclusion<'_>>,
) -> usize {
    let mut live = 0;
    for run in runs {
        if !run.status.is_live() {
            continue;
        }
        for node in run.nodes.values() {
            if !node_holds_session(&node.status) {
                continue;
            }
            let is_self = exclude.as_ref().is_some_and(|x| {
                x.run_id == run.run_id && x.node_id == node.node_id && x.iter == node.iter
            });
            if !is_self {
                live += 1;
            }
        }
    }
    live
}

/// Whether a node in the given status is currently holding a NodeRun tmux
/// session (and therefore consuming an admission slot).
///
/// `pub(crate)` so boot recovery can reuse the canonical "session-holding"
/// definition when reconciling dangling nodes of terminal runs (#215).
pub(crate) fn node_holds_session(status: &NodeStatus) -> bool {
    status.holds_session()
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
                    harness: None,
                    node_id: (*id).into(),
                    status: status.clone(),
                    iter: 1,
                    started_at: None,
                    completed_at: None,
                    failure_reason: None,
                    iterations: Vec::new(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                    missing_outputs: Vec::new(),
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
        // Kept as a single test to avoid a parallel-execution env-var race:
        // `SESSION_CAP_ENV` is process-global, so two tests mutating it
        // concurrently would flake. The stored-precedence assertions (#129,
        // ADR-0015) therefore live here too.
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

        // --- stored → env → default precedence (#129, ADR-0015) ---
        std::env::set_var(SESSION_CAP_ENV, "9");
        // Stored wins over env.
        assert_eq!(configured_cap_with(Some(30)), 30);
        // A zero/invalid stored value is ignored → falls through to env.
        assert_eq!(configured_cap_with(Some(0)), 9);
        // No stored value → env applies (identical to `configured_cap()`).
        assert_eq!(configured_cap_with(None), 9);
        // No stored and no env → default; stored still wins when the env is unset.
        std::env::remove_var(SESSION_CAP_ENV);
        assert_eq!(configured_cap_with(None), DEFAULT_SESSION_CAP);
        assert_eq!(configured_cap_with(Some(5)), 5);

        match saved {
            Some(v) => std::env::set_var(SESSION_CAP_ENV, v),
            None => std::env::remove_var(SESSION_CAP_ENV),
        }
    }

    // ── #489-C : the self-slot exclusion ─────────────────────────────────────

    fn run_with_node_at_iter(
        run_id: &str,
        node_id: &str,
        status: NodeStatus,
        iter: i64,
    ) -> RunState {
        let mut run = run_with_nodes(run_id, &[(node_id, status)]);
        run.nodes.get_mut(node_id).unwrap().iter = iter;
        run
    }

    /// The bug: a `restart_node` kills its own session, re-spawns the same
    /// iteration, and appends no lifecycle event in between — so the node still
    /// projects `Running` and, at `live == cap`, the restart is throttled against
    /// itself. Deterministically, and for good.
    #[test]
    fn the_slot_a_spawn_is_taking_back_is_not_counted_against_it() {
        let run = run_with_node_at_iter("r1", "impl", NodeStatus::Running, 1);
        assert_eq!(count_live_node_sessions([&run]), 1);
        assert_eq!(
            count_live_node_sessions_excluding(
                [&run],
                Some(SlotExclusion {
                    run_id: "r1",
                    node_id: "impl",
                    iter: 1,
                })
            ),
            0
        );
    }

    /// **The over-admission bug the key closes.** The count is global across Runs
    /// while node ids are local to a pipeline, so two concurrent Runs of the same
    /// pipeline both carry an `implementer` at `iter 1`. A `(node_id, iter)` key
    /// would discount the OTHER Run's live session and let the cap be exceeded —
    /// exactly the collapse this module exists to prevent.
    #[test]
    fn the_exclusion_never_discounts_another_runs_session() {
        let a = run_with_node_at_iter("run-a", "implementer", NodeStatus::Running, 1);
        let b = run_with_node_at_iter("run-b", "implementer", NodeStatus::Running, 1);
        assert_eq!(count_live_node_sessions([&a, &b]), 2);
        assert_eq!(
            count_live_node_sessions_excluding(
                [&a, &b],
                Some(SlotExclusion {
                    run_id: "run-b",
                    node_id: "implementer",
                    iter: 1,
                })
            ),
            1,
            "only run-b's own slot comes off the count"
        );
    }

    /// Without the `iter` condition the exclusion would be a free `+1` on any
    /// restart of a non-live iteration, i.e. a silent cap raise.
    #[test]
    fn the_exclusion_only_bites_on_the_live_iteration() {
        let run = run_with_node_at_iter("r1", "impl", NodeStatus::Running, 2);
        assert_eq!(
            count_live_node_sessions_excluding(
                [&run],
                Some(SlotExclusion {
                    run_id: "r1",
                    node_id: "impl",
                    iter: 1,
                })
            ),
            1,
            "iter 1 is not the live iteration: nothing to take back"
        );
    }

    /// A `Waiting` node holds no session, so there is nothing to exclude — and the
    /// exclusion must not conjure a slot out of it.
    #[test]
    fn excluding_a_session_less_node_changes_nothing() {
        let run = run_with_node_at_iter("r1", "impl", NodeStatus::Waiting, 1);
        assert_eq!(count_live_node_sessions([&run]), 0);
        assert_eq!(
            count_live_node_sessions_excluding(
                [&run],
                Some(SlotExclusion {
                    run_id: "r1",
                    node_id: "impl",
                    iter: 1,
                })
            ),
            0
        );
    }

    /// `None` is byte-for-byte the historical count — what `GET /sessions` and every
    /// other reader keeps calling, because an observability endpoint must report the
    /// TRUE count.
    #[test]
    fn no_exclusion_is_the_historical_count() {
        let r1 = run_with_nodes("r1", &[("a", NodeStatus::Running)]);
        let r2 = run_with_nodes(
            "r2",
            &[("b", NodeStatus::Running), ("c", NodeStatus::AwaitingUser)],
        );
        assert_eq!(
            count_live_node_sessions_excluding([&r1, &r2], None),
            count_live_node_sessions([&r1, &r2])
        );
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
