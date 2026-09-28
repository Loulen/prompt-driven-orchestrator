//! The daemon-wide facts the session context reads: the status bar's gauges,
//! the templates the library assistant writes, the harness it launches on.

use std::collections::HashMap;

use pdo_session::application::ports::{
    AssistantHarness, BoxFuture, DaemonGauges, TemplateSaveError, Templates,
};

use pdo_pipeline::domain::pipeline::{canonical_prompt_path, parse_pipeline};
use pdo_run::application::paths::resolve_pipeline_path;

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

impl Templates for RunSide {
    fn path_of(&self, pipeline_id: &str) -> std::path::PathBuf {
        resolve_pipeline_path(&self.repo_root, pipeline_id)
    }

    fn save(
        &self,
        pipeline_id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<std::path::PathBuf, TemplateSaveError> {
        // Same gate as `PUT /pipelines/{id}`: a template that does not parse must
        // not reach disk, where it breaks the canvas the user is looking at.
        if let Err(e) = parse_pipeline(yaml) {
            let (message, line) = pdo_pipeline::adapters::http::parse_error_to_structured(&e);
            return Err(TemplateSaveError::Invalid {
                error: format!("invalid YAML: {e}"),
                message,
                line,
            });
        }
        let path = resolve_pipeline_path(&self.repo_root, pipeline_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.self_writes.mark(&path);
        std::fs::write(&path, yaml)
            .map_err(|e| TemplateSaveError::Write(format!("write failed: {e}")))?;
        for (node_id, content) in prompts {
            let prompt_path = canonical_prompt_path(&path, node_id);
            if let Some(parent) = prompt_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            self.self_writes.mark(&prompt_path);
            if let Err(e) = std::fs::write(&prompt_path, content) {
                tracing::warn!("failed to write prompt for {node_id}: {e}");
            }
        }
        // Announce the write ourselves because self-writes are suppressed by the
        // watcher; otherwise the canvas would remain stale until reopened.
        let _ = self.bus_tx.send(serde_json::json!({
            "type": "pipeline_changed",
            "pipeline_id": pipeline_id,
            "path": path.to_string_lossy(),
        }));
        Ok(path)
    }
}

impl AssistantHarness for RunSide {
    /// Infra harness — no Run to follow: instance default, else the `claude`
    /// floor, resolved against the DISK TIER like the manager and the resolver.
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
