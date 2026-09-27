//! Reap a node's session on its iteration's terminal transition — an internal
//! transition of the Run, never authorized.

use std::path::Path;

use tracing::warn;

use crate::application::ports::Tmux;
use crate::domain::names::node_session_name;
use crate::domain::pane::pane_snapshot_path;

/// Persist the pane under the Run's node dir — so post-mortem inspection keeps
/// working after the session is gone (#205) — then kill the session. A
/// capture/write failure MUST still proceed to the kill, or a terminal node leaks
/// a live session toward the tmux-collapse point. Returns the session's name.
pub fn snapshot_and_kill(
    tmux: &dyn Tmux,
    repo_root: &Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> String {
    let session = node_session_name(run_id, node_id, iter);
    if let Some(content) = tmux.capture(&session) {
        let snapshot_path = pane_snapshot_path(repo_root, run_id, node_id, iter);
        if let Some(parent) = snapshot_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&snapshot_path, content) {
            warn!("reap: failed to persist pane snapshot for {session}: {e}");
        }
    }
    tmux.kill(&session);
    session
}
