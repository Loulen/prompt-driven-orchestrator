//! Duplicate a pipeline under the first free `(copy)` name.

use std::collections::HashSet;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::registry_names::{lenient_names, listed_name};
use super::Created;
use crate::domain::naming::{available_identity, copy_name};

impl Pipelines {
    /// A duplicate is a creation: the Opération names no target.
    pub fn duplicate(&self, actor: &Actor, id: &str) -> Result<Created, PipelineError> {
        self.authorize(actor, Operation::new(ResourceKind::Pipeline, Verb::Create))?;
        let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
        let parsed = crate::domain::pipeline::parse_pipeline(&yaml)?;
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        let files = self.registry.files();
        let listed: HashSet<String> = files.iter().map(listed_name).collect();
        let name = copy_name(&parsed.pipeline.name, &listed);
        let (new_id, new_name) = available_identity(&name, &lenient_names(&files), |stem| {
            self.registry.exists(stem)
        });
        let mut copy = parsed.pipeline;
        copy.name = new_name;
        let path = self
            .registry
            .publish(&new_id, &copy, &self.registry.prompts(id))
            .map_err(PipelineError::Storage)?;
        Ok(Created { id: new_id, path })
    }
}
