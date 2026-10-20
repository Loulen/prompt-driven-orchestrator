//! Overwrite a shared pipeline with a whole definition from elsewhere (a Run's
//! snapshot, ADR-0085), and the fingerprint the overwrite warning compares.

use std::collections::HashMap;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use crate::domain::fingerprint::fingerprint;
use crate::domain::pipeline::parse_pipeline;

impl Pipelines {
    /// Replace the pipeline `id` by `yaml` and `prompts`: the YAML, every
    /// prompt, and the prompts of the nodes `yaml` no longer has removed. The
    /// id stays: a `name:` the snapshot changed does not move the file. A
    /// gesture of an actor: it asks `pipeline` × `update`, as a save does.
    ///
    /// The write is the daemon's own, which the watcher does not announce:
    /// `pipeline_changed` goes out from here, so an open editor of `id`
    /// reloads.
    /// The file is a new version, saved by `actor`.
    pub async fn overwrite(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<(), PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Pipeline, Verb::Update).on(id),
        )?;
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        let replacement = parse_pipeline(yaml)?.pipeline;
        // The file as it was is a version before it is replaced.
        if let Err(e) = self.ensure_versions(id).await {
            tracing::warn!("versions of pipeline {id}: unreadable: {e:?}");
        }
        self.registry
            .write(id, yaml)
            .map_err(PipelineError::Storage)?;
        self.registry.write_prompts(id, prompts);
        self.registry.prune_prompts(id, &replacement);
        self.file_is_last_version(Some(actor), id, true).await;
        self.events
            .changed(id, &self.registry.path_of(id).to_string_lossy());
        info!("Pipeline {id} overwritten");
        Ok(())
    }

    /// The fingerprint of the pipeline `id` as it is on disk now (its YAML and
    /// its prompts); `None` when it cannot be read (deleted, renamed).
    pub fn fingerprint(&self, id: &str) -> Option<String> {
        let yaml = self.registry.read(id)?;
        Some(fingerprint(&yaml, &self.registry.prompts(id)))
    }
}
