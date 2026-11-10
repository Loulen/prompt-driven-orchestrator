//! The reap of a node's session once its iteration is over.

use std::sync::Arc;

use tracing::info;

use crate::application::sandbox as run_sandbox;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::application::supervision::detach_terminal_tail;
use crate::Runs;

/// Freeze a NodeRun's pane now, while the session that asked for its terminal
/// transition still waits for the answer. A `script` node's shell exits as soon as
/// `pdo complete` / `pdo fail` returns, so the detached reap would find it gone and
/// keep no pane (CORE-232); that reap re-captures if the session is still there.
pub fn freeze_node_pane(
    state: &Runs,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) {
    pdo_session::application::reap::snapshot_pane(state.tmux(), repo_root, run_id, node_id, iter);
}

/// Reap a NodeRun's tmux session on its terminal transition: the session
/// context snapshots its pane then kills it.
pub fn reap_node_session(
    state: &Runs,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    sandboxed: bool,
) {
    let session = pdo_session::application::reap::snapshot_and_kill(
        state.tmux(),
        repo_root,
        run_id,
        node_id,
        iter,
    );
    // Killing the tmux-side `docker exec` client leaves the container process
    // (reparented onto PID 1) alive, so double it with a targeted in-container kill
    // that scans `/proc` for this session's marker. No-op for `off`.
    run_sandbox::kill_session_best_effort(
        state.docker_cmd_override.as_deref().unwrap_or("docker"),
        sandboxed,
        run_id,
        &session,
    );
    info!("Reaped tmux session {session} on terminal transition");
}

/// A commit SHA cut to git's own abbreviation length, for a human-facing reason
/// string. Never for comparison — the full SHAs live in the event payload.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// Reap the session of a node whose completion just killed the Run.
///
/// Called from the refusal paths that park the Run — a merge-back conflict and a
/// failed delivery (#654). Those return a refusal *to a `pdo complete` running
/// inside the very session being killed*, so the kill has to outlive this request:
/// detached, exactly like `node_done`'s reap (#304, ADR-0023), or the response
/// never reaches the CLI and #490's exit codes lose their meaning.
///
/// Not called on the still-your-turn refusals (a missing output, a frontmatter
/// retry): those leave the Run alive and the agent free to fix and re-complete, so
/// its session is still doing something.
pub fn reap_dead_node_after_run_failure(
    state: &Arc<Runs>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    sandboxed: bool,
) {
    // The refusal ends a script's shell before the detached reap runs: freeze first.
    freeze_node_pane(state, repo_root, run_id, node_id, iter);
    let tail_state = state.clone();
    let tail_repo = repo_root.to_path_buf();
    let tail_run = run_id.to_string();
    let tail_node = node_id.to_string();
    detach_terminal_tail(
        "run_failed_reap",
        state.clone(),
        run_id.to_string(),
        node_id.to_string(),
        async move {
            reap_node_session(
                &tail_state,
                &tail_repo,
                &tail_run,
                &tail_node,
                iter,
                sandboxed,
            );
            // The run is dead, so its slot is free.
            retry_waiting_nodes(&tail_state).await;
        },
    );
}
