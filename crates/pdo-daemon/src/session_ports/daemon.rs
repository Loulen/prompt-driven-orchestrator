//! The daemon-wide facts the session context reads: the status bar's gauges,
//! the templates the library assistant writes, the harness it launches on.

use std::collections::HashMap;

use pdo_session::application::ports::{
    AssistantHarness, BoxFuture, DaemonGauges, TemplateSaveError, Templates,
};

use super::LegacySessionPorts;

impl DaemonGauges for LegacySessionPorts {
    fn live_sessions(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            match self.state() {
                Ok(state) => {
                    pdo_run::application::admission_count::count_global_live_sessions(&state.run)
                        .await
                }
                Err(_) => 0,
            }
        })
    }

    fn session_cap(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            let stored = match self.state() {
                Ok(state) => pdo_run::application::settings::stored_session_cap(&state.run).await,
                Err(_) => None,
            };
            pdo_run::adapters::env::configured_cap_with(stored)
        })
    }

    fn service_health(&self) -> serde_json::Value {
        self.state()
            .ok()
            .and_then(|state| serde_json::to_value(state.platform.service_health()).ok())
            .unwrap_or(serde_json::Value::Null)
    }
}

impl Templates for LegacySessionPorts {
    fn path_of(&self, pipeline_id: &str) -> std::path::PathBuf {
        match self.state() {
            Ok(state) => crate::resolve_pipeline_path(&state.repo_root, pipeline_id),
            Err(_) => std::path::PathBuf::new(),
        }
    }

    fn save(
        &self,
        pipeline_id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<std::path::PathBuf, TemplateSaveError> {
        let state = self.state().map_err(TemplateSaveError::Write)?;
        // Same gate as `PUT /pipelines/{id}`: a template that does not parse must
        // not reach disk, where it breaks the canvas the user is looking at.
        if let Err(e) = crate::pipeline::parse_pipeline(yaml) {
            let (message, line) = pdo_pipeline::adapters::http::parse_error_to_structured(&e);
            return Err(TemplateSaveError::Invalid {
                error: format!("invalid YAML: {e}"),
                message,
                line,
            });
        }
        let path = crate::resolve_pipeline_path(&state.repo_root, pipeline_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        state.recent_writes.mark(&path);
        std::fs::write(&path, yaml)
            .map_err(|e| TemplateSaveError::Write(format!("write failed: {e}")))?;
        for (node_id, content) in prompts {
            let prompt_path = crate::pipeline::canonical_prompt_path(&path, node_id);
            if let Some(parent) = prompt_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            state.recent_writes.mark(&prompt_path);
            if let Err(e) = std::fs::write(&prompt_path, content) {
                tracing::warn!("failed to write prompt for {node_id}: {e}");
            }
        }
        // Announce the write ourselves because self-writes are suppressed by the
        // watcher; otherwise the canvas would remain stale until reopened.
        let _ = state.bus_tx.send(serde_json::json!({
            "type": "pipeline_changed",
            "pipeline_id": pipeline_id,
            "path": path.to_string_lossy(),
        }));
        Ok(path)
    }
}

impl AssistantHarness for LegacySessionPorts {
    /// Infra harness — no Run to follow: instance default, else the `claude`
    /// floor, resolved against the DISK TIER like the manager and the resolver.
    fn resolve(&self) -> BoxFuture<'_, pdo_harness::registry::HarnessDescriptor> {
        Box::pin(async move {
            let Ok(state) = self.state() else {
                return pdo_harness::registry::claude();
            };
            let default_harness =
                pdo_run::application::settings::stored_default_harness(&state.run).await;
            let harness_name =
                pdo_harness::resolver::resolve_infra_harness(None, default_harness.as_deref());
            let home_root = crate::run_sandbox::sandbox_home_roots(&state.run)
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
