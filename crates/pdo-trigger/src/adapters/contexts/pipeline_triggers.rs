//! The Triggers as the run context reads them: which fire a pipeline, for the
//! warning before a Run overwrites it (ADR-0085).

use std::sync::Arc;

use pdo_run::application::ports::{BoxFuture, PipelineTrigger, PipelineTriggers};

use crate::application::ports::TriggerRepository;

/// The Triggers of the trigger context's store.
pub struct TriggersOfRuns(pub Arc<dyn TriggerRepository>);

impl PipelineTriggers for TriggersOfRuns {
    fn firing<'a>(
        &'a self,
        pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<PipelineTrigger>, String>> {
        Box::pin(async move {
            let triggers = self.0.list().await.map_err(|e| e.to_string())?;
            Ok(triggers
                .into_iter()
                .filter(|t| t.pipeline_id == pipeline_id)
                .map(|t| PipelineTrigger {
                    id: t.id,
                    name: t.name,
                    enabled: t.enabled,
                })
                .collect())
        })
    }
}
