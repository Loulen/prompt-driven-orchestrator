//! « Overwrite default pipeline » (ADR-0085): the one way a Run's edit reaches
//! the shared pipeline it was launched from. The Run records, at launch, which
//! pipeline it came from and its fingerprint; the warning reads what the
//! overwrite would touch; the overwrite hands the whole snapshot to the
//! pipeline context's registry.

use std::path::{Path, PathBuf};

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::application::operations::on_run;
use crate::application::paths::{
    effective_repo_root, run_scoped_pipeline_path, run_scoped_prompts_dir,
};
use crate::application::ports::{OverwriteRefusal, PipelineTrigger};
use crate::domain::projection::project;
use crate::domain::state::{RunState, RunStatus};
use crate::Runs;

/// The shared pipeline a Run was launched from, as it was then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSource {
    /// The id of the shared pipeline (its file stem in the registry).
    pub pipeline_id: String,
    /// Its fingerprint at launch (YAML and prompts).
    pub fingerprint: String,
}

/// Where a Run keeps its [`LaunchSource`], beside its snapshot.
pub fn launch_source_path(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("pipeline.source.json")
}

/// Record the shared pipeline at `pipeline_path` as the source of `run_id`,
/// with its fingerprint as the pipeline context's registry reads it. Best
/// effort: a Run without it only answers « unknown » to « changed since the
/// launch ».
pub(crate) fn record_launch_source(
    state: &Runs,
    repo_root: &Path,
    run_id: &str,
    pipeline_path: &Path,
) {
    let Some(pipeline_id) = pipeline_path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let Some(fingerprint) = state.source_pipelines.fingerprint(pipeline_id) else {
        return;
    };
    write_launch_source(
        repo_root,
        run_id,
        &LaunchSource {
            pipeline_id: pipeline_id.to_string(),
            fingerprint,
        },
    );
}

fn write_launch_source(repo_root: &Path, run_id: &str, source: &LaunchSource) {
    let path = launch_source_path(repo_root, run_id);
    let written = serde_json::to_vec(source)
        .map_err(|e| e.to_string())
        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
    if let Err(e) = written {
        warn!("run {run_id}: could not record the pipeline it was launched from: {e}");
    }
}

fn read_launch_source(repo_root: &Path, run_id: &str) -> Option<LaunchSource> {
    let bytes = std::fs::read(launch_source_path(repo_root, run_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// What « Overwrite default pipeline » would touch, read before the warning:
/// the body of `GET /runs/{run_id}/pipeline/overwrite-preview`.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct OverwritePreview {
    /// The shared pipeline the Run was launched from.
    pub pipeline_id: String,
    /// Whether it still exists (a rename or a delete leaves nothing to overwrite).
    pub pipeline_exists: bool,
    /// Whether it changed since the Run's launch; `None` when the Run did not
    /// record its launch (unknown, not « no »).
    pub modified_since_launch: Option<bool>,
    /// The Triggers that fire it, and will fire the overwritten version.
    pub triggers: Vec<PipelineTrigger>,
}

fn refusal(status: StatusCode, message: impl Into<String>) -> Box<Response> {
    let message = message.into();
    Box::new(
        (
            status,
            Json(serde_json::json!({ "error": message, "message": message })),
        )
            .into_response(),
    )
}

/// A non-archived Run, its repository and the shared pipeline it came from.
struct OverwriteTarget {
    repo_root: PathBuf,
    source: Option<LaunchSource>,
    pipeline_id: String,
}

impl Runs {
    async fn overwrite_target(&self, run_id: &str) -> Result<OverwriteTarget, Box<Response>> {
        let events = self.store().events(run_id).await.map_err(|e| {
            refusal(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("load events: {e}"),
            )
        })?;
        let Some(run_state) = project(&events) else {
            return Err(refusal(StatusCode::NOT_FOUND, "run not found"));
        };
        if run_state.status == RunStatus::Archived {
            return Err(refusal(
                StatusCode::CONFLICT,
                "an archived run is read-only: its pipeline cannot overwrite the default",
            ));
        }
        let repo_root = effective_repo_root(self, &run_state);
        if !run_scoped_pipeline_path(&repo_root, run_id).exists() {
            return Err(refusal(
                StatusCode::NOT_FOUND,
                "run-scoped pipeline not found",
            ));
        }
        let source = read_launch_source(&repo_root, run_id);
        let pipeline_id = source
            .as_ref()
            .map(|s| s.pipeline_id.clone())
            .unwrap_or_else(|| source_pipeline_id(&run_state));
        Ok(OverwriteTarget {
            repo_root,
            source,
            pipeline_id,
        })
    }

    /// `GET /runs/{run_id}/pipeline/overwrite-preview`: the shared pipeline the
    /// Run would overwrite, whether it changed since the launch, and the
    /// Triggers that fire it.
    pub async fn overwrite_preview(&self, run_id: &str) -> Response {
        let target = match self.overwrite_target(run_id).await {
            Ok(target) => target,
            Err(response) => return *response,
        };
        let triggers = match self.pipeline_triggers.firing(&target.pipeline_id).await {
            Ok(triggers) => triggers,
            Err(e) => {
                return *refusal(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("list triggers: {e}"),
                )
            }
        };
        let now = self.source_pipelines.fingerprint(&target.pipeline_id);
        let modified_since_launch = target
            .source
            .as_ref()
            .map(|source| now.as_deref() != Some(source.fingerprint.as_str()));
        Json(OverwritePreview {
            pipeline_exists: self.source_pipelines.exists(&target.pipeline_id),
            pipeline_id: target.pipeline_id,
            modified_since_launch,
            triggers,
        })
        .into_response()
    }

    /// `POST /runs/{run_id}/pipeline/overwrite-default`: the Run's whole
    /// snapshot (YAML and prompts) replaces the shared pipeline it was launched
    /// from. Asks `run` × `edit_pipeline`, then the pipeline context asks
    /// `pipeline` × `update`: both must allow it.
    pub async fn overwrite_default_pipeline(&self, actor: &Actor, run_id: &str) -> Response {
        if let Err(denied) = self
            .authorize(actor, &on_run("edit_pipeline", run_id))
            .await
        {
            return denied.into_response();
        }
        let target = match self.overwrite_target(run_id).await {
            Ok(target) => target,
            Err(response) => return *response,
        };
        let id = target.pipeline_id;
        let yaml =
            match std::fs::read_to_string(run_scoped_pipeline_path(&target.repo_root, run_id)) {
                Ok(yaml) => yaml,
                Err(e) => {
                    return *refusal(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("read run pipeline: {e}"),
                    )
                }
            };
        let prompts = pdo_pipeline::adapters::registry::read_prompts_dir(&run_scoped_prompts_dir(
            &target.repo_root,
            run_id,
        ));
        match self.source_pipelines.overwrite(actor, &id, &yaml, &prompts) {
            Ok(()) => {}
            Err(OverwriteRefusal::Denied(denied)) => return denied.into_response(),
            Err(OverwriteRefusal::NotFound) => {
                return *refusal(
                    StatusCode::NOT_FOUND,
                    format!("the default pipeline '{id}' no longer exists (renamed or deleted)"),
                )
            }
            Err(OverwriteRefusal::Failed(e)) => {
                return *refusal(StatusCode::UNPROCESSABLE_ENTITY, e)
            }
        }
        // The shared pipeline now IS this Run's snapshot: re-anchor the launch
        // fingerprint, so the next warning from this Run does not report its
        // own write as someone else's change.
        if let Some(now) = self.source_pipelines.fingerprint(&id) {
            write_launch_source(
                &target.repo_root,
                run_id,
                &LaunchSource {
                    pipeline_id: id.clone(),
                    fingerprint: now,
                },
            );
        }
        info!("Run {run_id} overwrote the default pipeline {id}");
        (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "pipeline_id": id })),
        )
            .into_response()
    }
}

/// The shared pipeline of a Run launched before it recorded its source: the
/// library id its launch carried, else its name.
fn source_pipeline_id(run_state: &RunState) -> String {
    run_state
        .pipeline_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| run_state.pipeline_name.clone())
}
