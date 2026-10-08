//! Delete a pipeline, its prompts and its validation.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;

impl Pipelines {
    /// Refused while a Run not yet completed uses it: its completion path
    /// resolves the pipeline by id.
    pub async fn delete(&self, actor: &Actor, id: &str) -> Result<(), PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Pipeline, Verb::Delete).on(id),
        )?;
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        let active = self.runs.using(id).await;
        if active > 0 {
            return Err(PipelineError::ActiveRuns(active));
        }
        self.registry.remove(id).map_err(PipelineError::Storage)?;
        self.forget_validation(id).await;
        info!("Deleted pipeline {id}");
        Ok(())
    }
}
