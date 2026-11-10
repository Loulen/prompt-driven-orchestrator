//! A node's pane (`GET /runs/{run_id}/nodes/{node_id}/pane`): the live session,
//! else the snapshot of a terminal or `Interrupted` iteration, else — for the
//! latest live iteration whose session died — a re-attach in place (ADR-0049
//! §3a). An `Interrupted` node is never re-attached by a read: it waits for a
//! human gesture.

use std::sync::Arc;
use std::time::Duration;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use crate::application::ports::{NoRunProjects, NodePanes, RunLookup, RunProjects, Tmux};
use crate::domain::names::node_session_name;
use crate::domain::pane::{pane_snapshot_path, Pane, PaneSource, ReattachOutcome};

/// Why no pane was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadPaneError {
    Run(RunLookup),
    /// The node is not in the Run.
    NodeNotFound,
    /// Resurrecting the session was refused.
    Refused(Denied),
}

pub struct ReadPane {
    panes: Arc<dyn NodePanes>,
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
}

impl ReadPane {
    pub fn new(
        panes: Arc<dyn NodePanes>,
        tmux: Arc<dyn Tmux>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            panes,
            tmux,
            host,
            projects: Arc::new(NoRunProjects),
        }
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.projects = projects;
        self
    }

    /// The pane of `(run, node, iter)`, and the name of its session.
    pub async fn execute(
        &self,
        actor: &Actor,
        run_id: &str,
        node_id: &str,
        iter: i64,
    ) -> Result<(String, Pane), ReadPaneError> {
        let target = self
            .panes
            .pane_target(run_id, node_id, iter)
            .await
            .map_err(ReadPaneError::Run)?
            .ok_or(ReadPaneError::NodeNotFound)?;
        let session = node_session_name(run_id, node_id, iter);
        let stale = !target.latest_iter;

        if let Some(content) = self.tmux.capture(&session) {
            return Ok((session, Pane::new(content, PaneSource::Live, stale)));
        }

        // Reaped terminal node: the session is gone but a pane snapshot was
        // persisted on the terminal transition. Never resurrect the session for a
        // terminal iteration — a pane request must not violate the
        // one-live-iteration invariant.
        let snapshot_path = pane_snapshot_path(&target.repo_root, run_id, node_id, iter);
        if target.terminal || target.interrupted {
            if let Ok(content) = std::fs::read_to_string(&snapshot_path) {
                return Ok((session, Pane::new(content, PaneSource::Snapshot, stale)));
            }
        }

        // ADR-0049: an `Interrupted` node comes back on a human gesture only
        // (`recover_node`, a restart). A read is not one, so it never relaunches
        // the harness: no snapshot, nothing to show.
        if target.interrupted {
            return Ok((session, unavailable(stale)));
        }

        if target.latest_iter && !target.terminal && !target.pending {
            // A re-attach IS a (re)spawn: the one mutation behind this read.
            let operation = Operation::new(ResourceKind::Session, Verb::Action("resume".into()))
                .on(session.clone())
                .in_projects(self.projects.projects_of_run(run_id).await);
            authorize(self.host.as_ref(), actor, &operation).map_err(ReadPaneError::Refused)?;
            let snapshot_or = |msg: &str| match std::fs::read_to_string(&snapshot_path) {
                Ok(c) => Pane::new(c, PaneSource::Snapshot, false),
                Err(_) => Pane::new(msg, PaneSource::Unavailable, false),
            };
            let outcome = self.panes.reattach(run_id, node_id, iter, &session).await;
            let pane = match outcome {
                // A script session can't be "continued" — snapshot it.
                ReattachOutcome::ScriptNode => snapshot_or("Script session no longer available"),
                // A harness with no resume mechanism serves its snapshot, never a
                // resume error — the same branch as a `script` node.
                ReattachOutcome::CannotResume => snapshot_or("Session no longer available"),
                // The frozen harness no longer resolves: refuse, naming it, rather
                // than relaunch `claude` in this node's worktree.
                ReattachOutcome::FrozenHarnessGone { harness } => {
                    warn!(
                        "node_pane: refusing to resurrect {node_id} iter {iter} in {run_id} — its \
                         frozen harness '{harness}' no longer resolves (embedded name dropped or disk \
                         descriptor removed); not relaunching claude in its place"
                    );
                    Pane::new(
                        format!(
                            "Session cannot be resumed: this node's harness '{harness}' no longer \
                             resolves. Restore its descriptor, or retry the node to start fresh."
                        ),
                        PaneSource::Unavailable,
                        false,
                    )
                }
                ReattachOutcome::TraceRefused { error } => {
                    warn!("node_pane: not resurrecting {node_id} iter {iter} in {run_id}: {error}");
                    snapshot_or("Session no longer available")
                }
                ReattachOutcome::CapReached { live, cap } => {
                    info!(
                        "node_pane: refusing to resurrect {node_id} iter {iter} in {run_id} \
                         — session cap reached ({live}/{cap})"
                    );
                    Pane::new(
                        format!("Session cannot be resumed: session cap reached ({live}/{cap})"),
                        PaneSource::Unavailable,
                        false,
                    )
                }
                ReattachOutcome::ResumeFailed { error } => {
                    warn!("Failed to resume session {session}: {error}");
                    Pane::new(
                        "Session no longer available",
                        PaneSource::Unavailable,
                        false,
                    )
                }
                ReattachOutcome::Resumed => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let content = self
                        .tmux
                        .capture(&session)
                        .unwrap_or_else(|| "Connecting...".to_string());
                    Pane::new(content, PaneSource::Resumed, false)
                }
                // Working dir gone → fall through to the placeholder below.
                ReattachOutcome::WorkingDirMissing => {
                    return Ok((session, unavailable(stale)));
                }
            };
            return Ok((session, pane));
        }

        Ok((session, unavailable(stale)))
    }
}

fn unavailable(stale: bool) -> Pane {
    Pane::new(
        "Session no longer available",
        PaneSource::Unavailable,
        stale,
    )
}
