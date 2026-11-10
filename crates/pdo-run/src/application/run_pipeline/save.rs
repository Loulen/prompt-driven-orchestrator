//! The mid-run edit of a Run's pipeline (« Save for this run », ADR-0085):
//! validated against what the Run's progress forbids (the pipeline context's
//! rules), written to the run-scoped copy only. The shared pipeline moves
//! through the explicit, warned overwrite ([`super::overwrite`]), never as a
//! side effect of a save.

use std::collections::HashMap;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use pdo_pipeline::domain::pipeline;
use serde::Deserialize;
use tracing::{info, warn};

use crate::application::operations::on_run;
use crate::application::paths::{
    effective_repo_root, run_scoped_pipeline_path, run_scoped_prompts_dir,
};
use crate::domain::projection::project;
use crate::domain::state::RunState;
use crate::Runs;

/// What the pipeline context's run-edit rules read of a Run: each node's
/// status, each loop's lap.
fn run_progress(run_state: &RunState) -> pdo_pipeline::domain::mutation::RunProgress {
    use crate::domain::state::NodeStatus as S;
    use pdo_pipeline::domain::mutation::{LoopLap, NodeProgress as P, RunProgress};
    RunProgress {
        nodes: run_state
            .nodes
            .iter()
            .map(|(id, node)| {
                let progress = match node.status {
                    S::Pending => P::Pending,
                    S::Waiting => P::Waiting,
                    S::Running => P::Running,
                    S::AwaitingUser => P::AwaitingUser,
                    S::Completed => P::Completed,
                    S::Skipped => P::Skipped,
                    S::Failed => P::Failed,
                    S::Stopped => P::Stopped,
                    S::Stale => P::Stale,
                    S::Interrupted => P::Interrupted,
                };
                (id.clone(), progress)
            })
            .collect(),
        laps: run_state
            .loop_states
            .iter()
            .map(|(id, lap)| {
                (
                    id.clone(),
                    LoopLap {
                        current_iter: lap.current_iter,
                        done: lap.done,
                    },
                )
            })
            .collect(),
    }
}

#[derive(Deserialize)]
pub struct SaveRunPipelineRequest {
    pub yaml: String,
    #[serde(default)]
    pub prompts: HashMap<String, String>,
}

impl Runs {
    /// `PUT /runs/{run_id}/pipeline`: edit the Run's pipeline mid-run, within
    /// what its progress allows. Writes the Run's snapshot only: neither the
    /// shared pipeline nor another Run moves. A gesture of an actor: it asks
    /// the Autorisation d'opération (`run` × `edit_pipeline`).
    pub async fn save_run_pipeline(
        &self,
        actor: &Actor,
        run_id: &str,
        req: SaveRunPipelineRequest,
    ) -> Response {
        if let Err(denied) = self
            .authorize(actor, &on_run("edit_pipeline", run_id))
            .await
        {
            return denied.into_response();
        }
        let state = self;
        let events = match state.store().events(run_id).await {
            Ok(e) => e,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("load events: {e}") })),
                )
                    .into_response();
            }
        };
        let run_state = match project(&events) {
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
        let yaml_path = run_scoped_pipeline_path(&repo_root, run_id);
        if !yaml_path.exists() {
            return (StatusCode::NOT_FOUND, "run-scoped pipeline not found").into_response();
        }

        let new_pipeline = match pipeline::parse_pipeline(&req.yaml) {
            Ok(r) => r.pipeline,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid YAML: {e}") })),
                )
                    .into_response();
            }
        };

        let old_yaml = match std::fs::read_to_string(&yaml_path) {
            Ok(y) => y,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("read old pipeline: {e}") })),
                )
                    .into_response();
            }
        };
        let old_pipeline = match pipeline::parse_pipeline(&old_yaml) {
            Ok(r) => r.pipeline,
            Err(_) => {
                // Old pipeline unparseable: skip validation, allow the overwrite.
                new_pipeline.clone()
            }
        };

        let rejections = pdo_pipeline::domain::mutation::validate_run_mutation(
            &old_pipeline,
            &new_pipeline,
            &run_progress(&run_state),
        );
        if !rejections.is_empty() {
            let details: Vec<_> = rejections
                .iter()
                .map(|r| serde_json::json!({ "node_id": r.node_id, "reason": r.reason }))
                .collect();
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "mutation rejected", "rejections": details })),
            )
                .into_response();
        }

        state.self_writes.mark(&yaml_path);
        if let Err(e) = std::fs::write(&yaml_path, &req.yaml) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("write failed: {e}") })),
            )
                .into_response();
        }

        let prompts_dir = run_scoped_prompts_dir(&repo_root, run_id);
        for (node_id, content) in &req.prompts {
            let prompt_path = prompts_dir.join(format!("{node_id}.md"));
            if let Some(parent) = prompt_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            state.self_writes.mark(&prompt_path);
            if let Err(e) = std::fs::write(&prompt_path, content) {
                warn!("failed to write run prompt for {node_id}: {e}");
            }
        }

        info!("Run-scoped pipeline for {run_id} saved (validated, snapshot only)");
        (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
    }
}
