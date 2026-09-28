//! The Run facts of the declared wait, of a node's pane, and of the run shell.

use pdo_session::application::ports::{
    BoxFuture, NodePanes, NodeWaits, RunLookup, RunShells, ShellTarget,
};
use pdo_session::domain::declared_wait::{
    AwaitingMarker, IterationFacts, NodeWaitFacts, WaitFacts,
};
use pdo_session::domain::launch::ContainerExec;
use pdo_session::domain::pane::{PaneTarget, ReattachOutcome};

use pdo_run::application::paths::effective_repo_root;
use pdo_run::application::sandbox::SandboxExec;
use pdo_run::domain::event::EventKind;
use pdo_run::domain::projection::project;
use pdo_run::domain::state::{NodeStatus, RunState, RunStatus};
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

use super::RunSide;

impl NodeWaits for RunSide {
    fn wait_facts<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<WaitFacts>, String>> {
        Box::pin(async move {
            let events = self.events(run_id).await.map_err(|e| e.to_string())?;
            Ok(project(&events).map(|run| wait_facts(&run, node_id)))
        })
    }

    fn record_awaiting<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.record(run_id, node_id, iter, EventKind::NodeAwaitingUser, payload))
    }

    fn record_resumed<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.record(run_id, node_id, iter, EventKind::NodeResumed, payload))
    }
}

/// What the projection says about one node, for a wait.
fn wait_facts(run: &RunState, node_id: &str) -> WaitFacts {
    WaitFacts {
        run_live: run.status.is_live(),
        defined: run.node_defs.iter().any(|d| d.id == node_id),
        is_script: run
            .node_defs
            .iter()
            .any(|d| d.id == node_id && d.node_type == "script"),
        node: run.nodes.get(node_id).map(|node| NodeWaitFacts {
            iter: node.iter,
            awaiting: node.status == NodeStatus::AwaitingUser,
            marker: node.awaiting.as_ref().map(|a| AwaitingMarker {
                cause: a.cause.clone(),
                message: a.message.clone(),
            }),
            iterations: node
                .iterations
                .iter()
                .map(|a| IterationFacts {
                    iter: a.iter,
                    holds_session: a.status.holds_session(),
                })
                .collect(),
        }),
    }
}

impl NodePanes for RunSide {
    fn pane_target<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, Result<Option<PaneTarget>, RunLookup>> {
        Box::pin(async move {
            let (runs, _, run_state) = self.projected(run_id).await?;
            let Some(node) = run_state.nodes.get(node_id) else {
                return Ok(None);
            };
            let terminal = node
                .iterations
                .iter()
                .find(|i| i.iter == iter)
                .is_some_and(|i| {
                    matches!(
                        i.status,
                        NodeStatus::Completed
                            | NodeStatus::Skipped
                            | NodeStatus::Failed
                            | NodeStatus::Stopped
                            | NodeStatus::Stale
                    )
                });
            Ok(Some(PaneTarget {
                latest_iter: node.iter == iter,
                terminal,
                pending: node.status == NodeStatus::Pending,
                repo_root: effective_repo_root(&runs, &run_state),
            }))
        })
    }

    fn reattach<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        session: &'a str,
    ) -> BoxFuture<'a, ReattachOutcome> {
        Box::pin(async move {
            let (runs, events, run_state) = match self.projected(run_id).await {
                Ok(p) => p,
                Err(RunLookup::NotFound) => {
                    return ReattachOutcome::TraceRefused {
                        error: format!("run {run_id} not found"),
                    }
                }
                Err(RunLookup::Storage(error)) => return ReattachOutcome::TraceRefused { error },
            };
            let repo_root = effective_repo_root(&runs, &run_state);
            pdo_run::application::node::reattach::reattach_node_session(
                &runs, &events, &run_state, &repo_root, run_id, node_id, iter, session,
            )
            .await
        })
    }
}

impl RunShells for RunSide {
    fn shell_target<'a>(
        &'a self,
        run_id: &'a str,
    ) -> BoxFuture<'a, Result<ShellTarget, RunLookup>> {
        Box::pin(async move {
            let (runs, _, run_state) = self.projected(run_id).await?;
            let repo_root = effective_repo_root(&runs, &run_state);
            let worktree = worktree_dir_for_run(&repo_root, run_id);
            let eligible = run_state.status.is_terminal()
                && run_state.status != RunStatus::Archived
                && worktree.exists();
            let sandbox = (!run_state.sandbox.is_off()).then(|| {
                // A run shell runs bash, not a harness tail: no staging-set env
                // to forward.
                Box::new(SandboxExec::new(
                    self.docker_cmd_override.as_deref(),
                    run_id,
                    &pdo_session::domain::names::shell_session_name(run_id),
                    &worktree,
                    &[],
                )) as Box<dyn ContainerExec>
            });
            Ok(ShellTarget {
                eligible,
                worktree,
                sandbox,
            })
        })
    }

    fn ensure_sandbox_ready<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let (runs, _, run_state) =
                self.projected(run_id).await.map_err(|e| format!("{e:?}"))?;
            let prep =
                match pdo_run::application::sandbox::context_from_state(&runs, &run_state).await {
                    Ok(prep) => tokio::task::spawn_blocking(move || prep.ensure_ready())
                        .await
                        .unwrap_or_else(|je| {
                            Err(anyhow::anyhow!("sandbox ensure_ready panicked: {je}"))
                        }),
                    Err(e) => Err(e),
                };
            prep.map_err(|e| format!("{e:#}"))
        })
    }
}
