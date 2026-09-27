//! The Run facts of the declared wait, of a node's pane, and of the run shell.

use pdo_session::application::ports::{
    BoxFuture, NodePanes, NodeWaits, RunLookup, RunShells, ShellTarget,
};
use pdo_session::domain::declared_wait::{
    AwaitingMarker, IterationFacts, NodeWaitFacts, WaitFacts,
};
use pdo_session::domain::launch::ContainerExec;
use pdo_session::domain::pane::{PaneTarget, ReattachOutcome};

use super::{LegacySessionPorts, SandboxExec};
use crate::event_log;

impl NodeWaits for LegacySessionPorts {
    fn wait_facts<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<WaitFacts>, String>> {
        Box::pin(async move {
            let state = self.state()?;
            let events = crate::load_events(&state.db, run_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(event_log::project(&events).map(|run| wait_facts(&run, node_id)))
        })
    }

    fn record_awaiting<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.record(
            run_id,
            node_id,
            iter,
            event_log::EventKind::NodeAwaitingUser,
            payload,
        ))
    }

    fn record_resumed<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.record(
            run_id,
            node_id,
            iter,
            event_log::EventKind::NodeResumed,
            payload,
        ))
    }
}

/// What the projection says about one node, for a wait.
fn wait_facts(run: &event_log::RunState, node_id: &str) -> WaitFacts {
    WaitFacts {
        run_live: run.status.is_live(),
        defined: run.node_defs.iter().any(|d| d.id == node_id),
        is_script: run
            .node_defs
            .iter()
            .any(|d| d.id == node_id && d.node_type == "script"),
        node: run.nodes.get(node_id).map(|node| NodeWaitFacts {
            iter: node.iter,
            awaiting: node.status == event_log::NodeStatus::AwaitingUser,
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

impl NodePanes for LegacySessionPorts {
    fn pane_target<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, Result<Option<PaneTarget>, RunLookup>> {
        Box::pin(async move {
            let (state, _, run_state) = self.projected(run_id).await?;
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
                        event_log::NodeStatus::Completed
                            | event_log::NodeStatus::Skipped
                            | event_log::NodeStatus::Failed
                            | event_log::NodeStatus::Stopped
                            | event_log::NodeStatus::Stale
                    )
                });
            Ok(Some(PaneTarget {
                latest_iter: node.iter == iter,
                terminal,
                pending: node.status == event_log::NodeStatus::Pending,
                repo_root: crate::effective_repo_root(&state, &run_state),
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
            let (state, events, run_state) = match self.projected(run_id).await {
                Ok(p) => p,
                Err(RunLookup::NotFound) => {
                    return ReattachOutcome::TraceRefused {
                        error: format!("run {run_id} not found"),
                    }
                }
                Err(RunLookup::Storage(error)) => return ReattachOutcome::TraceRefused { error },
            };
            let repo_root = crate::effective_repo_root(&state, &run_state);
            crate::reattach_node_session(
                &state, &events, &run_state, &repo_root, run_id, node_id, iter, session,
            )
            .await
        })
    }
}

impl RunShells for LegacySessionPorts {
    fn shell_target<'a>(
        &'a self,
        run_id: &'a str,
    ) -> BoxFuture<'a, Result<ShellTarget, RunLookup>> {
        Box::pin(async move {
            let (state, _, run_state) = self.projected(run_id).await?;
            let repo_root = crate::effective_repo_root(&state, &run_state);
            let worktree = crate::worktree_dir_for_run(&repo_root, run_id);
            let eligible = run_state.status.is_terminal()
                && run_state.status != event_log::RunStatus::Archived
                && worktree.exists();
            let sandbox = (!run_state.sandbox.is_off()).then(|| {
                // A run shell runs bash, not a harness tail: no staging-set env
                // to forward.
                Box::new(SandboxExec::new(
                    state.docker_cmd_override.as_deref(),
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
            let (state, _, run_state) =
                self.projected(run_id).await.map_err(|e| format!("{e:?}"))?;
            let prep = match crate::run_sandbox::context_from_state(&state, &run_state).await {
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
