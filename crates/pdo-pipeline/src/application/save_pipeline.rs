//! Save a pipeline from the editor: its YAML and the prompts it carries.

use std::collections::HashMap;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::Renamed;
use super::registry_names::lenient_name;
use crate::domain::naming::pipeline_slug;
use crate::domain::pipeline::parse_pipeline;

/// Where a save landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// The pipeline's id after the save.
    pub id: String,
    /// Whether the save changed the name, and so moved the file.
    pub renamed: bool,
}

impl Pipelines {
    /// The visible name and the file stem are 1:1 (#774): a save that changes
    /// `name:` moves the entry to the slug of the new name, with the refusals
    /// of a rename. Only a save that actually changes `name:` is a rename
    /// (#886): a stem that is not the slug of its name (`Mixed-Case.yaml`)
    /// stays where it is.
    pub async fn save(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<SaveOutcome, PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Pipeline, Verb::Update).on(id),
        )?;
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        let saved = parse_pipeline(yaml)?.pipeline;
        let new_name = saved.name.trim().to_string();
        let desired_id = pipeline_slug(&new_name);
        let on_disk = self.registry.read(id);
        let name_changed =
            on_disk.as_deref().and_then(lenient_name).as_deref() != Some(new_name.as_str());
        if !(name_changed && desired_id != id) {
            self.registry
                .write(id, yaml)
                .map_err(PipelineError::Storage)?;
            self.registry.write_prompts(id, prompts);
            self.registry.prune_prompts(id, &saved);
            info!("Pipeline {id} saved");
            return Ok(SaveOutcome {
                id: id.to_string(),
                renamed: false,
            });
        }
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        self.refuse_a_taken_identity(id, &new_name, &desired_id)
            .await?;
        // The visible name before the save: what Stats shows for the old id.
        let old_name = on_disk
            .as_deref()
            .and_then(|yaml| parse_pipeline(yaml).ok())
            .map(|parsed| parsed.pipeline.name.trim().to_string())
            .unwrap_or_else(|| id.to_string());
        let new_path = self
            .registry
            .relocate(id, &desired_id, yaml)
            .map_err(PipelineError::Storage)?;
        self.renames
            .record(&Renamed {
                old_id: id.to_string(),
                old_name,
                new_id: desired_id.clone(),
                new_name,
            })
            .await;
        self.carry_validation(id, &desired_id).await;
        self.registry.write_prompts(&desired_id, prompts);
        self.registry.prune_prompts(&desired_id, &saved);
        self.events
            .changed(&desired_id, &new_path.to_string_lossy());
        info!("Pipeline {id} saved as {desired_id} (name changed)");
        Ok(SaveOutcome {
            id: desired_id,
            renamed: true,
        })
    }
}
