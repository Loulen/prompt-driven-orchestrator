//! The daemon-wide facts the session context reads: the status bar's gauges,
//! the drafts of the templates the library assistant reads and saves into,
//! the harness it launches on.

use std::collections::HashMap;

use pdo_kernel::Actor;
use pdo_pipeline::application::PipelineError;
use pdo_run::application::paths::resolve_pipeline_path;
use pdo_session::application::ports::{
    AssistantHarness, BoxFuture, DaemonGauges, TemplateDraft, TemplateSaveError, TemplateSaved,
    Templates,
};

use super::RunSide;

impl DaemonGauges for RunSide {
    fn live_sessions(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            match self.runs.get() {
                Ok(runs) => {
                    pdo_run::application::admission_count::count_global_live_sessions(&runs).await
                }
                Err(_) => 0,
            }
        })
    }

    fn session_cap(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            let stored = match self.runs.get() {
                Ok(runs) => pdo_run::application::settings::stored_session_cap(&runs).await,
                Err(_) => None,
            };
            pdo_run::adapters::env::configured_cap_with(stored)
        })
    }

    fn service_health(&self) -> serde_json::Value {
        self.platform
            .get()
            .ok()
            .and_then(|platform| serde_json::to_value(platform.service_health()).ok())
            .unwrap_or(serde_json::Value::Null)
    }
}

/// ADR-0088: the assistant reads and saves the shared draft of the template,
/// never its file.
impl Templates for RunSide {
    fn path_of(&self, pipeline_id: &str) -> std::path::PathBuf {
        resolve_pipeline_path(&self.repo_root, pipeline_id)
    }

    fn read<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<TemplateDraft, TemplateSaveError>> {
        Box::pin(async move {
            let pipelines = self.pipelines.get().map_err(TemplateSaveError::Write)?;
            let draft = pipelines
                .assistant_read(actor, pipeline_id)
                .await
                .map_err(template_error)?;
            Ok(TemplateDraft {
                yaml: draft.yaml,
                prompts: draft.prompts,
            })
        })
    }

    fn save<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_id: &'a str,
        yaml: &'a str,
        prompts: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<TemplateSaved, TemplateSaveError>> {
        Box::pin(async move {
            let pipelines = self.pipelines.get().map_err(TemplateSaveError::Write)?;
            let saved = pipelines
                .assistant_save(actor, pipeline_id, yaml, prompts)
                .await
                .map_err(template_error)?;
            Ok(TemplateSaved {
                fields: saved.fields,
                version: saved.version.map(|v| v.number),
            })
        })
    }

    fn left(&self, actor: &Actor, pipeline_id: &str) {
        let Ok(pipelines) = self.pipelines.get() else {
            return;
        };
        let (actor, id) = (actor.clone(), pipeline_id.to_string());
        // Only the draft of `id`: a save into the next pipeline, made before
        // this runs, keeps its seat.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { pipelines.assistant_left(&actor, &id).await });
        }
    }
}

/// A refused read or save, in the session context's words. Same gate as
/// `PUT /pipelines/{id}`: a template that does not parse reaches no draft.
fn template_error(e: PipelineError) -> TemplateSaveError {
    match e {
        PipelineError::Invalid(e) => {
            let (message, line) = pdo_pipeline::adapters::http::parse_error_to_structured(&e);
            TemplateSaveError::Invalid {
                error: format!("invalid YAML: {e}"),
                message,
                line,
            }
        }
        PipelineError::NotFound => TemplateSaveError::NotFound,
        other => TemplateSaveError::Write(format!("{other:?}")),
    }
}

impl AssistantHarness for RunSide {
    /// Infra harness — no Run to follow: instance default, else the `claude`
    /// floor, resolved against the DISK TIER like the manager.
    fn resolve(&self) -> BoxFuture<'_, pdo_harness::registry::HarnessDescriptor> {
        Box::pin(async move {
            let Ok(runs) = self.runs.get() else {
                return pdo_harness::registry::claude();
            };
            let default_harness =
                pdo_run::application::settings::stored_default_harness(&runs).await;
            let harness_name =
                pdo_harness::resolver::resolve_infra_harness(None, default_harness.as_deref());
            let home_root = pdo_run::application::sandbox::sandbox_home_roots(&runs)
                .map(|(home, _)| home)
                .unwrap_or_default();
            pdo_harness::adapters::descriptor_file::load(&home_root)
                .resolve(&harness_name)
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "library assistant: unknown harness '{harness_name}' — launching on the claude floor"
                    );
                    pdo_harness::registry::claude()
                })
        })
    }
}
