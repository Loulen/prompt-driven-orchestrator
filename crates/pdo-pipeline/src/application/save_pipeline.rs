//! Save a pipeline from the editor: its YAML and the prompts it carries,
//! frozen as a version (ADR-0088).

use std::collections::HashMap;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::Renamed;
use super::registry_names::lenient_name;
use super::Created;
use crate::domain::draft::DraftContent;
use crate::domain::naming::{pipeline_slug, rewrite_pipeline_name_field};
use crate::domain::pipeline::parse_pipeline;
use crate::domain::version::{next_version, PipelineVersionRef, SaveAs};

/// Where a save landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// The pipeline's id after the save.
    pub id: String,
    /// Whether the save changed the name, and so moved the file.
    pub renamed: bool,
    /// The version the file now is.
    pub version: PipelineVersionRef,
    /// The pipeline the version saved until now was kept as.
    pub kept: Option<Created>,
}

impl Pipelines {
    /// The visible name and the file stem are 1:1 (#774): a save that changes
    /// `name:` moves the entry to the slug of the new name, with the refusals
    /// of a rename. Only a save that actually changes `name:` is a rename
    /// (#886): a stem that is not the slug of its name (`Mixed-Case.yaml`)
    /// stays where it is.
    ///
    /// A draft of the pipeline takes the saved file in, field by field. The
    /// file is a new version.
    pub async fn save(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<SaveOutcome, PipelineError> {
        let saved = self
            .save_version(actor, id, yaml, prompts, &SaveAs::new_version())
            .await?;
        if saved.renamed {
            self.draft_moved(id, &saved.id, true).await;
        } else {
            self.draft_file_changed(&saved.id).await;
        }
        self.announce_saved(&saved);
        Ok(saved)
    }

    /// Restore the version `number` of `id` (ADR-0088): its YAML and its
    /// prompts saved as the next version, as the editor's Restore then Save
    /// does. The name stays the pipeline's current one: a version from before
    /// a rename does not rename the pipeline back.
    pub async fn restore_version(
        &self,
        actor: &Actor,
        id: &str,
        number: u32,
    ) -> Result<SaveOutcome, PipelineError> {
        let version = self.version(id, number).await?;
        let yaml = self
            .detail(id)
            .ok()
            .and_then(|current| rewrite_pipeline_name_field(&version.yaml, &current.pipeline.name))
            .unwrap_or(version.yaml);
        self.save(actor, id, &yaml, &version.prompts).await
    }

    /// Save the draft of `id` as the editor serialized it (ADR-0088), as
    /// `save_as` says: a new version, or the last one replaced, and the
    /// version saved until now kept as a pipeline of its own. `draft` is what
    /// the draft held then, the file it counts its changes from now on.
    pub async fn save_draft(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
        draft: &DraftContent,
        save_as: &SaveAs,
    ) -> Result<SaveOutcome, PipelineError> {
        let saved = self.save_version(actor, id, yaml, prompts, save_as).await?;
        self.draft_saved(id, &saved.id, draft).await;
        self.announce_saved(&saved);
        Ok(saved)
    }

    /// Write the file, then freeze it as the version `save_as` says. One
    /// save at a time: two at once would take the same number. The version
    /// saved until now is kept as a pipeline before the write: one that
    /// cannot be kept fails the save with nothing written.
    async fn save_version(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
        save_as: &SaveAs,
    ) -> Result<SaveOutcome, PipelineError> {
        let _one_at_a_time = self.version_writes.lock().await;
        if save_as.keep_old {
            self.authorize(
                actor,
                Operation::new(ResourceKind::Pipeline, Verb::Update).on(id),
            )?;
        }
        let last = match self.registry.exists(id) {
            true => self.ensure_versions(id).await?.pop(),
            false => None,
        };
        let old = match (&last, save_as.keep_old) {
            (Some(last), true) => self
                .versions
                .get(id, last.number)
                .await
                .map_err(PipelineError::Storage)?,
            _ => None,
        };
        let kept = match old {
            Some(old) => Some(self.keep_as_pipeline(actor, &old).await?),
            None => None,
        };
        let (renamed, saved_id) = self.write_saved(actor, id, yaml, prompts).await?;
        if renamed {
            self.carry_versions(id, &saved_id).await;
        }
        let (number, title) = next_version(last.as_ref(), save_as);
        let version = self
            .record_version(Some(actor), &saved_id, number, title)
            .await;
        Ok(SaveOutcome {
            id: saved_id,
            renamed,
            version,
            kept,
        })
    }

    /// The lists learn the new version from here: the save is the daemon's
    /// own write, which the watcher does not announce, and an editor that
    /// saved must not reload it (#17).
    fn announce_saved(&self, saved: &SaveOutcome) {
        self.events.version_changed(&saved.id);
    }

    /// Write the file: whether the name moved it, and its id after.
    async fn write_saved(
        &self,
        actor: &Actor,
        id: &str,
        yaml: &str,
        prompts: &HashMap<String, String>,
    ) -> Result<(bool, String), PipelineError> {
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
            return Ok((false, id.to_string()));
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
        Ok((true, desired_id))
    }
}
