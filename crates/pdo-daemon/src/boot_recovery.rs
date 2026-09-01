//! Boot recovery — reconcile persisted run state against the live process world
//! at daemon startup.
//!
//! Behavior (#213 / #215): after a daemon restart the event log may still claim
//! nodes are `Running`/`AwaitingUser` whose tmux sessions died with the previous
//! process (or whose whole tmux server collapsed). Left alone such a node stays
//! `Running` forever, burning an admission slot (#202). At boot [`run_boot_recovery`]
//! detects each divergence and reconciles it fail-fast through the transition
//! guard (via `append_event`), never silently auto-repairing:
//!   - a terminal run still projecting a session-holding node (#215) → `Failed`;
//!   - an orphaned live node whose tmux session is gone → `Failed`;
//!   - a sub-worktree branch merged into the pipeline branch with no
//!     `NodeCompleted` (#213 AC3) → surfaced (logged), never fabricated complete;
//!   - a run-level stall reconciled via the shared `reconcile_run_level_stall`.
//!
//! Non-reentrancy (ADR-0009): this is a linear sequence of guarded `append_event`
//! calls and must never call the scheduler or re-enter itself. `reconcile_run_level_stall`
//! and `retry_waiting_nodes` live in `lib.rs`; this module calls **up** into them,
//! never into the scheduler directly.

use tracing::{error, warn};

use crate::worktree_ops::sub_worktree_branch;
use crate::{admission, event_log, tmux_session_manager};
use crate::{
    append_event, effective_repo_root, load_all_run_ids, load_events, reconcile_run_level_stall,
    retry_waiting_nodes, AppState,
};

/// Reconcile persisted run state against the live process world at daemon boot.
///
/// Posture: fail-fast, never silent auto-repair — every reconciliation routes
/// through the transition guard (#212, via [`append_event`]).
pub(crate) async fn run_boot_recovery(state: &AppState) {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("Boot recovery: failed to load run ids: {e}");
            return;
        }
    };

    let socket = state.tmux_socket();

    for run_id in &run_ids {
        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let run_state = match event_log::project(&events) {
            Some(s) => s,
            None => continue,
        };

        // (0) Terminal run still projecting a session-holding node (#215).
        // Fail-fast can mark the whole run Failed while a sibling node is still
        // Running, so a terminal run can survive a restart with an inconsistent
        // projection. Reconcile each dangling node, then skip the live-run
        // handling below — the run is terminal and must stay so.
        // Deliberately NOT `RunStatus::is_terminal()`: this set omits `Skipped`,
        // whose handling at boot is an open question (#237 follow-up F1).
        let run_terminal = matches!(
            run_state.status,
            event_log::RunStatus::Completed
                | event_log::RunStatus::Failed
                | event_log::RunStatus::Halted
                | event_log::RunStatus::Archived
        );
        if run_terminal {
            let dangling: Vec<(String, i64, event_log::NodeStatus)> = run_state
                .nodes
                .iter()
                .filter(|(_, ns)| admission::node_holds_session(&ns.status))
                .map(|(id, ns)| (id.clone(), ns.iter, ns.status.clone()))
                .collect();
            for (node_id, iter, node_status) in &dangling {
                let session = tmux_session_manager::node_session_name(run_id, node_id, *iter);
                let interrupted = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeInterrupted,
                    node_id: Some(node_id.clone()),
                    iter: Some(*iter),
                    payload: Some(serde_json::json!({
                        "reason": format!(
                            "boot_recovery: run is {:?} (terminal) but node left \
                             session-holding ({:?}) across a daemon restart \
                             (session {session})",
                            run_state.status, node_status
                        )
                    })),
                };
                // Through the guard: `validate_interrupt` returns NoOp once the
                // iteration is terminal, so a second boot pass appends nothing.
                // `finalize` does not lift a terminal run to AwaitingUser, so this
                // only frees the phantom session-holding slot.
                if let Err(e) = append_event(state, &interrupted).await {
                    error!(
                        "Boot recovery: failed to reconcile dangling {node_id} iter {iter} \
                         in terminal run {run_id}: {e}"
                    );
                } else {
                    warn!(
                        "Boot recovery: node {node_id} iter {iter} in terminal run {run_id} \
                         left session-holding ({node_status:?}) — marked Interrupted"
                    );
                }
            }
            continue; // terminal run: orphan/stall handling below does not apply
        }

        if run_state.status != event_log::RunStatus::Running
            && run_state.status != event_log::RunStatus::AwaitingUser
        {
            continue;
        }

        // #407 D10: a live sandboxed Run needs its container back after a daemon
        // restart — reconcile it here, BEFORE the orphan scan. `spawn_blocking`
        // because `ensure_ready` may build/probe docker.
        //
        // #432 (ADR-0031 §7): keep the two `Err` arms SPLIT — they fail for
        // categorically different reasons (see each arm).
        if !run_state.sandbox.is_off() {
            match crate::sandbox_run::context_from_state(state, &run_state).await {
                Ok(ctx) => {
                    match tokio::task::spawn_blocking(move || crate::sandbox_run::ensure_ready(&ctx))
                        .await
                    {
                        // #445: record the container as ready, or the spawn precondition
                        // refuses every node of a Run whose prep task died with the
                        // previous daemon (projection frozen at `pending`, nothing else
                        // lifts it). Success arm only — the `warn!` arms below leave it
                        // `pending` on purpose, deferring rather than spawning into a
                        // container that is not there. Gated on the Run actually being
                        // blocked, else every already-`ready` Run appends a no-op event
                        // per boot.
                        Ok(Ok(())) => {
                            if run_state.sandbox_spawn_block().is_some() {
                                crate::mark_sandbox_prep_ready(state, run_id).await;
                            }
                        }
                        // Don't make this arm fatal: `ensure_ready` touches the Docker
                        // socket, and `service_unit.rs` emits `After=network-online.target`
                        // WITHOUT `After=docker.service`, so a systemd-restarted daemon
                        // can reach this before `dockerd` accepts connections — a fatal
                        // arm would mass-`RunFailed` every live sandboxed Run on that
                        // boot-ordering race. (Fix `After=docker.service` first.)
                        Ok(Err(e)) => warn!(
                            "Boot recovery: failed to ensure sandbox container for run {run_id}: {e:#}"
                        ),
                        Err(je) => warn!(
                            "Boot recovery: sandbox ensure_ready panicked for run {run_id}: {je}"
                        ),
                    }
                }
                // FATAL since #432: an unresolvable FROZEN staging selection, which
                // never touches the Docker socket — so it cannot fail transiently, and
                // the next boot would fail identically for ever. A Run left `Running`
                // with a home nobody can stage is worse than a Run that says so.
                Err(e) => {
                    crate::fail_run_sandbox_prep(
                        state,
                        run_id,
                        &format!("sandbox prep failed at boot recovery: {e:#}"),
                    )
                    .await;
                    continue;
                }
            }
        }

        // (1) Orphaned live nodes: live status, no tmux session.
        let orphaned: Vec<(String, i64)> = run_state
            .nodes
            .iter()
            .filter(|(_, ns)| {
                matches!(
                    ns.status,
                    event_log::NodeStatus::Running | event_log::NodeStatus::AwaitingUser
                )
            })
            .filter_map(|(id, ns)| {
                let session = tmux_session_manager::node_session_name(run_id, id, ns.iter);
                (!tmux_session_manager::session_exists(&socket, &session))
                    .then(|| (id.clone(), ns.iter))
            })
            .collect();

        for (node_id, iter) in &orphaned {
            let session = tmux_session_manager::node_session_name(run_id, node_id, *iter);
            let interrupted = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeInterrupted,
                node_id: Some(node_id.clone()),
                iter: Some(*iter),
                payload: Some(serde_json::json!({
                    "reason": format!(
                        "session_died: tmux session {session} no longer exists \
                         (daemon restarted while the node held a session)"
                    )
                })),
            };
            // Since résilience (ADR-0049) a node lost across a daemon restart is
            // `Interrupted`, not `Failed`: "la session est morte, pas le travail".
            // The run parks `AwaitingUser` (derived in `finalize`), never `Failed`.
            // Through the guard: a node that turned terminal organically is a no-op.
            if let Err(e) = append_event(state, &interrupted).await {
                error!("Boot recovery: failed to interrupt orphaned {node_id} iter {iter}: {e}");
            } else {
                warn!(
                    "Boot recovery: node {node_id} iter {iter} in run {run_id} \
                     orphaned (session {session} gone) — marked Interrupted"
                );
            }
        }

        // (2) Merged-without-event divergence: a sub-worktree branch merged into
        // the pipeline branch whose node has no NodeCompleted. Surface it.
        let repo_root = effective_repo_root(state, &run_state);
        detect_merged_without_event(&repo_root, run_id, &run_state);

        // (3) #214: run-level stall — `Running` with no live node and nothing
        // schedulable, which (1) does not cover. Must run AFTER (1): it re-reads
        // fresh state so it sees the interrupts appended there.
        reconcile_run_level_stall(state, run_id).await;
    }

    // (4) #509: interrupting an orphan in (1) frees its admission slot.
    // `reconcile_run_level_stall` re-drives the queue only for a Run it reconciles
    // terminal, and a Run whose orphan died while a SIBLING is still `Running`
    // never stalls at the run level — so its freed slot would never be
    // redistributed, and other Runs' queued nodes starve across the restart,
    // invisible to `run_stall_reason` and to `stale_detector` alike.
    // `retry_waiting_nodes` has no timer of its own (all callers are event-driven,
    // #159), so this one restart-time sweep is what closes the gap. Global and
    // idempotent: once, after the whole loop.
    retry_waiting_nodes(state).await;
}

/// Detect sub-worktree branches whose work was merged into the pipeline branch
/// but for which no `NodeCompleted` was recorded (event log / git divergence,
/// #213 AC3). Logged as a fail-fast warning — never silently reconciled.
fn detect_merged_without_event(
    repo_root: &std::path::Path,
    run_id: &str,
    run_state: &event_log::RunState,
) {
    let pipeline_branch = format!("pdo/run-{run_id}");
    let divergent = merged_without_event_nodes(run_id, run_state, |sub_branch| {
        branch_is_merged_into(repo_root, sub_branch, &pipeline_branch)
    });
    for (node_id, sub_branch, status) in divergent {
        warn!(
            "Boot recovery: sub-worktree branch {sub_branch} is merged into \
             {pipeline_branch} but node {node_id} has no NodeCompleted \
             (status {status:?}) — git/event-log divergence in run {run_id}"
        );
    }
}

/// Pure detection of the git/event-log divergence in #213 AC3: an **isolated**
/// node (#653) — one owning a sub-worktree branch — that is **not** marked
/// `Completed` in the event log, yet whose branch `is_merged` reports as merged
/// into the pipeline branch. Returns `(node_id, sub_branch, status)` triples.
///
/// `is_merged` is injected so this is testable without a real git repo.
fn merged_without_event_nodes<F>(
    run_id: &str,
    run_state: &event_log::RunState,
    is_merged: F,
) -> Vec<(String, String, event_log::NodeStatus)>
where
    F: Fn(&str) -> bool,
{
    let mut out = Vec::new();
    for (node_id, ns) in &run_state.nodes {
        // #653/ADR-0060: a sub-branch exists iff the node is isolated. Read off
        // the Run snapshot (this sweep holds no event log): a node whose frozen
        // value differs has an `is_merged` probe answer of its own anyway — an
        // absent branch simply never reports merged.
        if !crate::snapshot_isolation(run_state, node_id) {
            continue;
        }
        // #620: a `Skipped` node is settled and never held a sub-branch — exclude
        // it from merge-recovery exactly as a `Completed` one.
        if ns.status.is_settled_complete() {
            continue;
        }
        let sub_branch = sub_worktree_branch(run_id, node_id, ns.iter);
        if is_merged(&sub_branch) {
            out.push((node_id.clone(), sub_branch, ns.status.clone()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Whether `branch` has been merged into `into` (i.e. `branch`'s tip is an
/// ancestor of `into`). Best-effort: a missing branch / non-repo returns false.
fn branch_is_merged_into(repo_root: &std::path::Path, branch: &str, into: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", branch, into])
        .current_dir(repo_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberately duplicated from lib.rs's test module, which still needs its own
    // copy for the stall tests. Do not remove the lib.rs copy.
    fn run_state_with_node(
        run_id: &str,
        node_id: &str,
        isolated: bool,
        status: event_log::NodeStatus,
        iter: i64,
    ) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "test".into());
        rs.node_defs.push(event_log::NodeDefInfo {
            isolated_worktree: Some(isolated),
            id: node_id.into(),
            name: None,
            node_type: "agent".into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });
        rs.nodes.insert(
            node_id.into(),
            event_log::NodeState {
                isolated_worktree: None,
                harness: None,
                cost: None,
                node_id: node_id.into(),
                status,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
            },
        );
        rs
    }

    // Deliberately duplicated from worktree_ops.rs's test module. Do not move.
    fn init_test_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "initial"]);
    }

    #[test]
    fn merged_without_event_flags_a_merged_uncompleted_isolated_node() {
        // #213 AC3: an isolated node whose sub-worktree branch is merged but
        // which never recorded a NodeCompleted is a git/event-log divergence.
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            true,
            event_log::NodeStatus::Running,
            1,
        );
        let divergent = merged_without_event_nodes("20260101-120000-abc", &rs, |_branch| true);
        assert_eq!(
            divergent.len(),
            1,
            "the merged uncompleted node must be flagged"
        );
        assert_eq!(divergent[0].0, "impl");
        assert_eq!(divergent[0].1, "pdo/sub-20260101-120000-abc-impl-iter-1");
    }

    #[test]
    fn merged_without_event_ignores_completed_node() {
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            true,
            event_log::NodeStatus::Completed,
            1,
        );
        let divergent = merged_without_event_nodes("20260101-120000-abc", &rs, |_branch| true);
        assert!(divergent.is_empty(), "a completed node is not a divergence");
    }

    #[test]
    fn merged_without_event_ignores_unmerged_and_shared_worktree() {
        // A non-isolated node owns no sub-worktree branch; an unmerged branch is fine.
        let doc = run_state_with_node(
            "20260101-120000-abc",
            "doc",
            false,
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &doc, |_| true).is_empty());

        let isolated = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            true,
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &isolated, |_| false).is_empty());
    }

    // Exercises the real `git merge-base --is-ancestor` path that the
    // closure-injected tests above stub out.
    #[test]
    fn branch_is_merged_into_tracks_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_test_repo(root);

        let run_id = "20260101-120000-abc";
        let pipeline_branch = format!("pdo/run-{run_id}");
        let sub_branch = sub_worktree_branch(run_id, "impl", 1);

        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
        };

        git(&["branch", &pipeline_branch]);
        git(&["checkout", "-b", &sub_branch]);
        std::fs::write(root.join("work.txt"), "node work\n").unwrap();
        git(&["add", "work.txt"]);
        git(&["commit", "-m", "node work"]);
        git(&["checkout", &pipeline_branch]);
        git(&["merge", "--no-ff", "--no-edit", &sub_branch]);

        assert!(
            branch_is_merged_into(root, &sub_branch, &pipeline_branch),
            "a merged sub-branch must be an ancestor of the pipeline branch"
        );

        git(&["checkout", &sub_branch]);
        std::fs::write(root.join("more.txt"), "extra\n").unwrap();
        git(&["add", "more.txt"]);
        git(&["commit", "-m", "extra unmerged work"]);
        assert!(
            !branch_is_merged_into(root, &sub_branch, &pipeline_branch),
            "a sub-branch with commits beyond the merge point is not fully merged"
        );

        assert!(
            !branch_is_merged_into(root, "pdo/sub-does-not-exist", &pipeline_branch),
            "a missing branch is reported unmerged, not an error"
        );
    }
}
