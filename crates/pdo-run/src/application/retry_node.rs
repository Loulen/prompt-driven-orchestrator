//! `RetryNode`: the canvas's Retry button, and what a retry would invalidate.

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info};

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::events::reload_run_state;
use crate::application::node::{primitives as node_primitives, spawn as node_spawn};
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_pipeline_path;
use crate::application::paths::run_scoped_pipeline_path;
use crate::application::reopen::embed_reopen_for_targeted_command;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::domain::{graph_resolver, loop_region, retry_verdict, transition_guard};
use crate::domain::{projection, state};
use crate::Runs;
use pdo_kernel::Actor;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// `RetryNode`: the operator retries a node (the canvas's Retry button): its
/// downstream is invalidated and it spawns again, reopening a terminal Run.
#[derive(Debug, Clone)]
pub struct RetryNode {
    pub run_id: String,
    pub node_id: String,
}

impl Runs {
    /// Retry / Play (`POST /runs/{id}/nodes/{node}/retry`, the canvas Retry button).
    ///
    /// Three things hold this handler together, and each is load-bearing:
    ///
    /// 1. **Head probe** ([`transition_guard::retry_run_precondition`]) — the FIRST
    ///    gesture, before the stop and before the two `invalidate_nodes`. Being first is
    ///    the point: `NodeInvalidated` is not a lifecycle transition, so the guard never
    ///    refuses it, and a probe placed any later would leave the self-invalidation
    ///    committed — freezing the node `pending` for ever. The predicate is
    ///    run-liveness ALONE, not a synthetic `NodeStarted`: retry re-spawns a
    ///    Running/Completed node, both of which `validate_start` refuses.
    /// 2. **Route the (re)spawn through [`node_spawn::spawn_node`]**, the reference
    ///    primitive (ADR-0009 addendum), not the legacy `node_primitives::start_node`,
    ///    so the transition guard, the sandbox precondition, the atomic admission cap and
    ///    the orphan-reap-on-abort all come for free.
    /// 3. **Map the [`node_spawn::SpawnOutcome`] truthfully** ([`retry_verdict`]) rather
    ///    than answering an unconditional `200`. The success body keeps the historical
    ///    `{iter, invalidated}` root contract the canvas reads.
    pub async fn retry_node(self: &Arc<Self>, actor: &Actor, command: RetryNode) -> Response {
        let RetryNode { run_id, node_id } = command;
        if let Err(denied) = self.authorize(actor, &on_node("retry", &run_id, &node_id)) {
            return denied.into_response();
        }
        let state = self;

        let events = match load_events(state, &run_id).await {
            Ok(e) => e,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        let run_state = match projection::project(&events) {
            Some(s) => s,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "run not found" })),
                )
                    .into_response();
            }
        };

        // Captured BEFORE any invalidation OR re-open: both the self-invalidation below
        // and the re-open's interrupted-node drop remove the node from the projection, so
        // re-deriving the iter afterwards would read 1 and collide branch names.
        let current_iter = run_state.nodes.get(&node_id).map(|ns| ns.iter).unwrap_or(1);

        // A retry on a terminal (or incident-parked) Run re-opens it atomically here,
        // BEFORE the liveness probe below sees it. The human's retry IS the re-open
        // gesture (ADR-0009 amended).
        let run_state = match embed_reopen_for_targeted_command(state, &run_id, run_state).await {
            Ok(s) => s,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response()
            }
        };

        // HEAD PROBE 1 — run-liveness. The FIRST gesture: a refusal here leaves ZERO side
        // effects. The iter feeds the refusal template only, so the pre-parse
        // `current_iter + 1` is fine.
        if let Some(reason) =
            transition_guard::retry_run_precondition(&run_state, &node_id, current_iter + 1)
        {
            let refusal = retry_verdict::RetryRefusal::RetryRejected {
                message: reason.to_string(),
                session_killed: false,
            };
            info!(
                "node_retry refused for {node_id} in run {run_id}: {}",
                refusal.reason()
            );
            return crate::adapters::http::verdicts::retry_response(
                &retry_verdict::RetryVerdict::Refused(refusal),
            );
        }

        // HEAD PROBE 2 — the sandbox precondition, also before any side effect: a
        // not-`ready` container would make the spawn `docker exec` into a name that does
        // not exist yet. Re-checked inside `spawn_node`.
        if let Some(reason) = run_state.sandbox_spawn_block() {
            let refusal = retry_verdict::RetryRefusal::SandboxPrepNotReady {
                message: reason,
                session_killed: false,
            };
            info!(
                "node_retry deferred for {node_id} in run {run_id}: {}",
                refusal.reason()
            );
            return crate::adapters::http::verdicts::retry_response(
                &retry_verdict::RetryVerdict::Refused(refusal),
            );
        }

        let repo_root = effective_repo_root(state, &run_state);
        let pipeline_path = {
            let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
            if run_scoped.exists() {
                run_scoped
            } else {
                resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
            }
        };
        let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "cannot read pipeline" })),
            )
                .into_response();
        };
        let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "cannot parse pipeline" })),
            )
                .into_response();
        };
        let pipeline_def = parse_result.pipeline;

        // HEAD PROBE 3 — the target must be in the Run's pipeline snapshot, since
        // `spawn_node` needs the `NodeDef`. Also pre-side-effect, so an unknown id is a
        // clean refusal, never a session killed for nothing.
        let Some(node) = pipeline_def.nodes.iter().find(|n| n.id == node_id).cloned() else {
            let refusal = retry_verdict::RetryRefusal::NodeNotFound {
                node_id: node_id.clone(),
            };
            info!("node_retry refused in run {run_id}: {}", refusal.reason());
            return crate::adapters::http::verdicts::retry_response(
                &retry_verdict::RetryVerdict::Refused(refusal),
            );
        };

        let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

        // FROM HERE ON THERE ARE SIDE EFFECTS: the Run is provably live, the sandbox is
        // ready, and the target exists.

        if run_state
            .nodes
            .get(&node_id)
            .is_some_and(|ns| ns.status == state::NodeStatus::Running)
        {
            let stop_params = node_primitives::StopNodeParams {
                run_id: &run_id,
                node_id: &node_id,
                iter: current_iter,
                tmux: state.tmux(),
            };
            let stop_result = node_primitives::stop_node(&stop_params);
            for ev in &stop_result.events {
                if let Err(e) = append_event(state, ev).await {
                    error!("failed to append stop event: {e}");
                }
            }
        }

        // Retry is loop-aware, because a node's `iter` inside a bounded region IS the lap
        // index:
        //  ① a region member re-runs the SAME lap, never `current_iter + 1` — bumping it
        //     would forge a phantom lap and drag the region toward its `max_iter`;
        //  ② the downstream walk skips the region's re-entry edges, so it resets only the
        //     lap's forward slice, never an earlier member of the same lap whose output is
        //     already validated.
        let next_iter = if loop_region::bounded_region_for_member(&pipeline_def, &node_id).is_some()
        {
            current_iter
        } else {
            current_iter + 1
        };

        let excluded_edges = loop_region::bounded_reentry_edges(&pipeline_def);
        let downstream: Vec<String> =
            graph_resolver::downstream_subgraph_excluding(&pipeline_def, &node_id, &excluded_edges)
                .into_iter()
                .collect();

        let inv_params = node_primitives::InvalidateNodesParams {
            run_id: &run_id,
            node_ids: &downstream,
            artifacts_dir: &artifacts_dir,
        };
        let inv_result = node_primitives::invalidate_nodes(&inv_params);
        for ev in &inv_result.events {
            if let Err(e) = append_event(state, ev).await {
                error!("failed to append invalidation event: {e}");
            }
        }

        let self_inv = node_primitives::InvalidateNodesParams {
            run_id: &run_id,
            node_ids: std::slice::from_ref(&node_id),
            artifacts_dir: &artifacts_dir,
        };
        let self_inv_result = node_primitives::invalidate_nodes(&self_inv);
        for ev in &self_inv_result.events {
            if let Err(e) = append_event(state, ev).await {
                error!("failed to append self-invalidation event: {e}");
            }
        }

        // Reload so `resolve_run_variables` sees the invalidated state. `spawn_node`
        // re-projects internally for its guard/cap, but not for these variables.
        let events = match load_events(state, &run_id).await {
            Ok(e) => e,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        let resolved_vars = resolve_run_variables(&pipeline_def, &events);

        // Route the (re)spawn through the reference primitive: every guarantee
        // `force_spawn_node` re-codes by hand is carried here.
        let spawn_ctx = node_spawn::SpawnContext {
            pipeline: &pipeline_def,
            run_id: &run_id,
            pipeline_path: &pipeline_path,
            worktree_dir: &worktree_dir,
            artifacts_dir: &artifacts_dir,
            resolved_vars: &resolved_vars,
            repo_root: &repo_root,
        };
        let outcome = node_spawn::spawn_node(
            node_spawn::SpawnDeps::from_state(state),
            &spawn_ctx,
            &node,
            next_iter,
        )
        .await;

        let mut invalidated: Vec<String> = downstream;
        invalidated.sort();

        let verdict = match outcome {
            node_spawn::SpawnOutcome::Spawned {
                reused_sub_worktree,
                base_sha,
                interrupted_git_ops,
            } => retry_verdict::RetryVerdict::Spawned {
                node_id: node_id.clone(),
                iter: next_iter,
                invalidated,
                reused_sub_worktree,
                base_sha,
                interrupted_git_ops,
            },
            // A `NodeWaiting` was appended and `retry_waiting_nodes` genuinely re-drives
            // it: a 2xx, and NOT a noop (ADR-0037 §2).
            node_spawn::SpawnOutcome::Throttled => retry_verdict::RetryVerdict::Waiting {
                reason: format!(
                    "node {node_id} iter {next_iter} is queued behind the session cap: it will \
                 spawn when a slot frees — do not re-issue"
                ),
                invalidated,
            },
            // Both are races: the head probes passed, then `spawn_node` re-evaluated
            // against a fresher projection. A running node was stopped + invalidated
            // first, hence `session_killed`.
            node_spawn::SpawnOutcome::Deferred { reason } => retry_verdict::RetryVerdict::Refused(
                retry_verdict::RetryRefusal::SandboxPrepNotReady {
                    message: reason,
                    session_killed: true,
                },
            ),
            node_spawn::SpawnOutcome::Refused { reason } => {
                retry_verdict::RetryVerdict::Refused(retry_verdict::RetryRefusal::RetryRejected {
                    message: reason,
                    session_killed: true,
                })
            }
            // A breakdown, not a verdict → 500. `run_failed` is re-PROJECTED, never
            // guessed: the producers of `Failed` disagree on whether `RunFailed` is on the
            // log, and a 500 routes the CLI toward `pdo fail` — bad advice if it already is.
            node_spawn::SpawnOutcome::Failed { reason } => {
                let run_failed = reload_run_state(state, &run_id)
                    .await
                    .is_some_and(|(_, s)| s.status == state::RunStatus::Failed);
                retry_verdict::RetryVerdict::Broken {
                    message: reason,
                    run_failed,
                }
            }
        };

        info!("node_retry: {node_id} iter {next_iter} in run {run_id} -> {verdict:?}");
        crate::adapters::http::verdicts::retry_response(&verdict)
    }
}

/// What a retry of `node_id` would invalidate (a read: it asks no
/// authorization).
pub async fn retry_preview(state: &Runs, run_id: String, node_id: String) -> Response {
    let events = match load_events(state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match projection::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot read pipeline" })),
        )
            .into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot parse pipeline" })),
        )
            .into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    // Must mirror `node_retry`'s loop-aware invalidation exactly, or the preview
    // promises something the retry will not do.
    let excluded_edges = loop_region::bounded_reentry_edges(&pipeline_def);
    let mut downstream: Vec<String> =
        graph_resolver::downstream_subgraph_excluding(&pipeline_def, &node_id, &excluded_edges)
            .into_iter()
            .collect();
    downstream.sort();

    let with_artifacts: Vec<&String> = downstream
        .iter()
        .filter(|nid| artifacts_dir.join(nid.as_str()).exists())
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "downstream": downstream,
            "affected_count": with_artifacts.len(),
            "with_artifacts": with_artifacts,
        })),
    )
        .into_response()
}
