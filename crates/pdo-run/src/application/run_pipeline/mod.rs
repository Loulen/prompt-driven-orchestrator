//! A Run's own pipeline (its run-scoped copy, or the durable one once the Run
//! is archived): read as the editor reads it, exported as a portable document
//! with its skills sidecar, edited mid-run for this Run only ([`save`]), and
//! copied over the shared pipeline it came from ([`overwrite`], ADR-0085).

mod overwrite;
mod save;

use std::collections::HashMap;

use axum::extract::Json;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use pdo_pipeline::domain::pipeline;

pub use overwrite::{launch_source_path, LaunchSource, OverwritePreview};
pub(crate) use overwrite::record_launch_source;
pub use save::SaveRunPipelineRequest;

use crate::application::paths::{
    archived_or_live_pipeline_yaml, archived_or_live_prompts_dir, run_scoped_pipeline_path,
    run_scoped_prompts_dir,
};
use crate::domain::projection::project;
use crate::Runs;

impl Runs {
    /// `GET /runs/{run_id}/pipeline`: the pipeline a Run runs (or ran), its
    /// prompts and its parse diagnostics.
    pub async fn run_pipeline(&self, run_id: &str) -> Response {
        let state = self;
        // Resolve against the durable store when the run is archived — its repo-local
        // worktree and pipeline.yaml are gone — against the live run dir otherwise.
        let run_state = match state.store().events(run_id).await {
            Ok(events) => project(&events),
            Err(_) => None,
        };
        let yaml_path = match &run_state {
            Some(rs) => archived_or_live_pipeline_yaml(state, rs, run_id),
            None => run_scoped_pipeline_path(&state.repo_root, run_id),
        };
        let yaml = match std::fs::read_to_string(&yaml_path) {
            Ok(y) => y,
            Err(_) => {
                return (StatusCode::NOT_FOUND, "run-scoped pipeline not found").into_response();
            }
        };

        let parse_result = match pipeline::parse_pipeline(&yaml) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("parse error: {e}") })),
                )
                    .into_response();
            }
        };

        let prompts_dir = match &run_state {
            Some(rs) => archived_or_live_prompts_dir(state, rs, run_id),
            None => run_scoped_prompts_dir(&state.repo_root, run_id),
        };
        let mut prompts: HashMap<String, String> = HashMap::new();
        if prompts_dir.is_dir() {
            for entry in std::fs::read_dir(&prompts_dir)
                .into_iter()
                .flatten()
                .flatten()
            {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let (Some(stem), Ok(content)) = (
                        p.file_stem().and_then(|s| s.to_str()),
                        std::fs::read_to_string(&p),
                    ) {
                        prompts.insert(stem.to_string(), content);
                    }
                }
            }
        }

        Json(serde_json::json!({
            "id": run_id,
            "scope": "run",
            "path": yaml_path.to_string_lossy(),
            "yaml": yaml,
            "pipeline": parse_result.pipeline,
            "prompts": prompts,
            "diagnostics": parse_result.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>(),
        }))
        .into_response()
    }
}

/// The pipeline a Run ran (archived or live) with its prompts — what the Run's
/// portable document and its skills sidecar are both built from.
async fn run_pipeline_for_export(
    state: &Runs,
    run_id: &str,
) -> Result<(pipeline::PipelineDef, HashMap<String, String>), Box<Response>> {
    let run_state = state
        .store()
        .events(run_id)
        .await
        .ok()
        .and_then(|events| project(&events));
    let yaml_path = match &run_state {
        Some(run) => archived_or_live_pipeline_yaml(state, run, run_id),
        None => run_scoped_pipeline_path(&state.repo_root, run_id),
    };
    let yaml = std::fs::read_to_string(&yaml_path)
        .map_err(|_| Box::new((StatusCode::NOT_FOUND, "run pipeline not found").into_response()))?;
    let parsed = pipeline::parse_pipeline(&yaml).map_err(|error| {
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("invalid run pipeline: {error}") })),
            )
                .into_response(),
        )
    })?;
    let prompts_dir = match &run_state {
        Some(run) => archived_or_live_prompts_dir(state, run, run_id),
        None => run_scoped_prompts_dir(&state.repo_root, run_id),
    };
    Ok((
        parsed.pipeline,
        pdo_pipeline::adapters::registry::read_prompts_dir(&prompts_dir),
    ))
}

impl Runs {
    /// `GET /runs/{run_id}/pipeline/document/skills`: the skills sidecar of the
    /// Run's pipeline.
    pub async fn run_pipeline_skills(&self, run_id: &str) -> Response {
        match run_pipeline_for_export(self, run_id).await {
            Ok((pipeline, _)) => pdo_pipeline::adapters::http::skills_sidecar_response(
                pdo_pipeline::application::sidecar_export(self.bank.as_ref(), &pipeline),
            ),
            Err(response) => *response,
        }
    }

    /// `GET /runs/{run_id}/pipeline/document`: the Run's pipeline as a portable
    /// document.
    pub async fn run_pipeline_document(&self, run_id: &str) -> Response {
        let (pipeline, prompts) = match run_pipeline_for_export(self, run_id).await {
            Ok(found) => found,
            Err(response) => return *response,
        };
        match pdo_pipeline::domain::portable::export(&pipeline, &prompts) {
            Ok(document) => (
                [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
                document,
            )
                .into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error })),
            )
                .into_response(),
        }
    }
}
