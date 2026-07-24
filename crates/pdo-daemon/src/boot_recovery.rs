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
//! Carve (#276): this concern was extracted verbatim from `lib.rs` behind the
//! single entry point [`run_boot_recovery`]. Non-reentrancy (ADR-0009): it is a
//! linear sequence of guarded `append_event` calls and must never call the
//! scheduler or re-enter itself. The shared `reconcile_run_level_stall` (used by
//! both boot recovery and the stale sweep) stays in `lib.rs`; this module calls
//! up into it.

use tracing::{error, warn};

use crate::worktree_ops::sub_worktree_branch;
use crate::{admission, event_log, tmux_session_manager};
use crate::{
    append_event, effective_repo_root, find_node_type, load_all_run_ids, load_events,
    reconcile_run_level_stall, AppState,
};

/// Reconcile persisted run state against the live process world at daemon boot.
///
/// Posture: fail-fast, never silent auto-repair. After a daemon restart the
/// event log may claim nodes are `Running`/`AwaitingUser` whose tmux sessions
/// died with the previous process (or whose whole tmux server collapsed). Such
/// a node would otherwise stay `Running` forever, burning an admission slot
/// (#202). At boot we detect each one — its session is absent on our socket —
/// and transition it to `Failed` with a cause naming the orphaned session,
/// through the transition guard (#212, via [`append_event`]).
///
/// A second divergence class — a sub-worktree branch merged into the pipeline
/// branch with no corresponding `NodeCompleted` event — is detected and
/// surfaced (logged) so the operator sees the inconsistency; it is not
/// silently completed (that would fabricate a transition the agent never made).
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
        // Running, so a terminal run can survive a restart with a node the
        // projection shows as Running/AwaitingUser. Phase 1 already excludes it
        // from the session cap, but the projection stays inconsistent until we
        // reconcile it. Fail each dangling node at its current iter, routed
        // through the guard (so a second boot pass is a clean no-op), then skip
        // the live-run handling below — the run is terminal and must stay so.
        // NOTE: deliberately NOT `RunStatus::is_terminal()` — this set omits
        // `Skipped`. Whether a `Skipped` run at boot should route through
        // dangling-node reconciliation is an open question (#237 follow-up F1).
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
                let fail = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeFailed,
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
                // Through the guard: idempotent across reboots. validate_fail
                // returns NoOp once the iteration is already terminal, so a
                // second pass appends nothing.
                if let Err(e) = append_event(state, &fail).await {
                    error!(
                        "Boot recovery: failed to reconcile dangling {node_id} iter {iter} \
                         in terminal run {run_id}: {e}"
                    );
                } else {
                    warn!(
                        "Boot recovery: node {node_id} iter {iter} in terminal run {run_id} \
                         left session-holding ({node_status:?}) — marked Failed"
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
        // restart — reconcile it here (before the orphan scan). Synchronous effect
        // via spawn_blocking (ensure_ready may build/probe docker); failure is
        // logged, not fatal — the orphan/stall handling below still runs, and a
        // downstream spawn would fail loud on a still-missing container. No-op for
        // `off`.
        if !run_state.sandbox.is_off() {
            match crate::sandbox_run::context_from_state(state, &run_state) {
                Ok(ctx) => {
                    match tokio::task::spawn_blocking(move || crate::sandbox_run::ensure_ready(&ctx))
                        .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => warn!(
                            "Boot recovery: failed to ensure sandbox container for run {run_id}: {e:#}"
                        ),
                        Err(je) => warn!(
                            "Boot recovery: sandbox ensure_ready panicked for run {run_id}: {je}"
                        ),
                    }
                }
                Err(e) => {
                    warn!("Boot recovery: failed to build sandbox context for run {run_id}: {e:#}")
                }
            }
        }

        // (1) Orphaned live nodes: Running/AwaitingUser with no tmux session.
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
            let fail = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeFailed,
                node_id: Some(node_id.clone()),
                iter: Some(*iter),
                payload: Some(serde_json::json!({
                    "reason": format!(
                        "boot_recovery: tmux session {session} no longer exists \
                         (node was Running across a daemon restart)"
                    )
                })),
            };
            // Through the guard: if the node turned terminal organically before
            // this pass, the failure is dropped as a no-op.
            if let Err(e) = append_event(state, &fail).await {
                error!("Boot recovery: failed to fail orphaned {node_id} iter {iter}: {e}");
            } else {
                warn!(
                    "Boot recovery: node {node_id} iter {iter} in run {run_id} \
                     orphaned (session {session} gone) — marked Failed"
                );
            }
        }

        // (2) Merged-without-event divergence: a sub-worktree branch merged into
        // the pipeline branch whose node has no NodeCompleted. Surface it.
        let repo_root = effective_repo_root(state, &run_state);
        detect_merged_without_event(&repo_root, run_id, &run_state);

        // (3) #214: run-level stall. A run can survive a crash as `Running` with
        // no live node and nothing schedulable — either no node ever spawned, or
        // (1) just failed an orphan whose downstream can never run. Boot recovery
        // for nodes (1) does not cover this run-level case; reconcile it terminal
        // here so the run never stays Running forever. Re-reads fresh state so it
        // sees any orphan failure appended in (1).
        reconcile_run_level_stall(state, run_id).await;
    }
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

/// Pure detection of the git/event-log divergence in #213 AC3: a node owning a
/// sub-worktree branch (`code-mutating` / `merge`) that is **not** marked
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
        let node_type = find_node_type(run_state, node_id);
        if !matches!(node_type, Some("code-mutating") | Some("merge")) {
            continue;
        }
        if matches!(ns.status, event_log::NodeStatus::Completed) {
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

    // Duplicated from lib.rs's test module (its stall/`run_stall_reason` tests
    // still need it) — a tiny pure `RunState`/`NodeState` builder, no `AppState`,
    // no DB. Deliberate carve-boundary duplication (cf. worktree_ops.rs's
    // `init_test_repo`). Do not remove the lib.rs copy.
    fn run_state_with_node(
        run_id: &str,
        node_id: &str,
        node_type: &str,
        status: event_log::NodeStatus,
        iter: i64,
    ) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "test".into());
        rs.node_defs.push(event_log::NodeDefInfo {
            id: node_id.into(),
            name: None,
            node_type: node_type.into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });
        rs.nodes.insert(
            node_id.into(),
            event_log::NodeState {
                node_id: node_id.into(),
                status,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
            },
        );
        rs
    }

    // Duplicated from worktree_ops.rs's test module — a 14-line
    // `git init/config/add/commit` fixture. Do not move.
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
    fn merged_without_event_flags_a_merged_uncompleted_code_node() {
        // #213 AC3: a code-mutating node whose sub-worktree branch is merged but
        // which never recorded a NodeCompleted is a git/event-log divergence.
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
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
        // A merged branch WITH a NodeCompleted is the normal, consistent case.
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
            event_log::NodeStatus::Completed,
            1,
        );
        let divergent = merged_without_event_nodes("20260101-120000-abc", &rs, |_branch| true);
        assert!(divergent.is_empty(), "a completed node is not a divergence");
    }

    #[test]
    fn merged_without_event_ignores_unmerged_and_doc_only() {
        // Doc-only nodes own no sub-worktree branch; an unmerged branch is fine.
        let doc = run_state_with_node(
            "20260101-120000-abc",
            "doc",
            "doc-only",
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &doc, |_| true).is_empty());

        let cm = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &cm, |_| false).is_empty());
    }

    // Per-module coverage for the git probe that the closure-injected tests above
    // stub out (#276 AC: new per-module unit tests). Exercises the real
    // `git merge-base --is-ancestor` path over a scratch repo.
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

        // Pipeline branch off the initial commit; a sub-worktree branch adds a
        // commit and is then merged back into the pipeline branch.
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

        // A commit added to the sub-branch after the merge is no longer contained
        // in the pipeline branch, so the sub-branch tip is no longer an ancestor.
        git(&["checkout", &sub_branch]);
        std::fs::write(root.join("more.txt"), "extra\n").unwrap();
        git(&["add", "more.txt"]);
        git(&["commit", "-m", "extra unmerged work"]);
        assert!(
            !branch_is_merged_into(root, &sub_branch, &pipeline_branch),
            "a sub-branch with commits beyond the merge point is not fully merged"
        );

        // A branch that does not exist is best-effort false, not a panic.
        assert!(
            !branch_is_merged_into(root, "pdo/sub-does-not-exist", &pipeline_branch),
            "a missing branch is reported unmerged, not an error"
        );
    }
}
