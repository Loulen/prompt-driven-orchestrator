//! How an aborted spawn is recorded: before `NodeStarted` (the orphan reaped,
//! the node interrupted) or after it (the node failed, the Run interrupted).

use std::path::PathBuf;

use tracing::error;

use super::*;
use crate::application::events::append_event_with;
use crate::domain::event;
use pdo_workspace::adapters::git::worktrees::reap_orphan_sub_worktree;

/// Interrupt a run when a node spawn aborts *before* `NodeStarted` is appended
/// (#279 / #498, ADR-0050 §1). Reaps any orphaned sub-worktree + branch the
/// spawn created, then appends a visible cause naming the node.
///
/// Since résilience (ADR-0049) the cause is `NodeInterrupted`, **not**
/// `RunFailed`: a spawn abort is an infra incident, not a business failure, so
/// the runtime never terminalises the run. The guard's `validate_interrupt`
/// admits a `NodeInterrupted` even for an iteration that never opened a
/// `NodeStarted` row, so the projection materialises the node `Interrupted` and
/// [`finalize`](crate::event_log) parks the run `AwaitingUser` with this reason
/// — visible, recoverable, never frozen `running` (the #498 trap). This is why
/// the pre-résilience code had to reach for the un-guarded `RunFailed`: a
/// `NodeFailed` on a never-started node was a guard no-op.
pub(super) async fn interrupt_spawn_before_start(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    orphan: Option<&(PathBuf, String)>,
    reason: &str,
) {
    interrupt_spawn_before_start_with_code(
        deps,
        repo_root,
        run_id,
        node_id,
        iter,
        orphan,
        "spawn_aborted",
        reason,
    )
    .await
}

/// [`interrupt_spawn_before_start`] with an explicit machine `reason_code` (#601):
/// `spawn_aborted` for a PDO-side abort, [`HARNESS_BINARY_MISSING_CODE`] when the
/// sandbox image lacks the harness binary (#708) — a distinct code, because the fix
/// is the user's image, not a retry.
#[allow(clippy::too_many_arguments)]
pub(super) async fn interrupt_spawn_before_start_with_code(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    orphan: Option<&(PathBuf, String)>,
    code: &str,
    reason: &str,
) {
    error!("Run {run_id}: node {node_id} spawn aborted before NodeStarted — {code}: {reason}");
    if let Some((sub_worktree_dir, sub_branch)) = orphan {
        reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
    }
    let interrupted = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeInterrupted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        // #601: prefix the machine slug so the node reason is `<code>: <prose>`
        // and [`finalize`](crate::event_log) derives `awaiting_reason_code`
        // (`spawn_aborted`, `harness_binary_missing`). `source: "spawn"` is retained
        // for existing readers.
        payload: Some(serde_json::json!({
            "reason": format!("{code}: {reason}"),
            "reason_code": code,
            "source": "spawn",
        })),
    };
    if let Err(e) = append_event_with(deps.runs, &interrupted).await {
        error!("Run {run_id}: failed to append NodeInterrupted after spawn abort: {e}");
    }
}

/// Interrupt a run when a node spawn aborts *after* `NodeStarted` is appended
/// (#508). The iteration is already `Running`, so a `NodeInterrupted` is a legal
/// transition and IS appended: it moves the *node* to `Interrupted` (non
/// terminal, ADR-0049), closing the window where the liveness sweep would
/// rewrite it `Failed` with a false `session_died` cause and where `GET …/pane`
/// / boot_recovery could re-drive a `Running` node that has no session. The run
/// then parks `AwaitingUser` (derived in [`finalize`](crate::event_log)), never
/// `RunFailed`. The reap is gated on `orphan` (Some iff THIS spawn created the
/// sub-worktree — a reuse must destroy nothing; ADR-0037 §6). The order
/// (node-interrupt → reap) mirrors the #488 "terminal event first, reap second"
/// convention.
pub(super) async fn interrupt_spawn_after_start(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    orphan: Option<&(PathBuf, String)>,
    reason: &str,
) {
    error!("Run {run_id}: node {node_id} spawn failed after NodeStarted — {reason}");

    let interrupted = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeInterrupted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        // #601: machine slug prefix, as in `interrupt_spawn_before_start`.
        payload: Some(serde_json::json!({
            "reason": format!("spawn_aborted: {reason}"),
            "reason_code": "spawn_aborted",
            "source": "spawn",
        })),
    };
    if let Err(e) = append_event_with(deps.runs, &interrupted).await {
        error!("Run {run_id}: failed to append NodeInterrupted after spawn failure: {e}");
    }

    // Reap ONLY what this spawn created (gated; ADR-0037 §6 — a reuse loses
    // nothing).
    if let Some((sub_worktree_dir, sub_branch)) = orphan {
        reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
    }
}
