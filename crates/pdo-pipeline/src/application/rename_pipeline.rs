//! Rename a pipeline (#774): the visible name and the file stem move
//! together. The YAML is never round-tripped: only its top-level `name:` line
//! is spliced (#682), so comments and unknown keys survive.

use std::path::PathBuf;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::Renamed;
use super::registry_names::listed_name;
use crate::domain::naming::{pipeline_slug, rewrite_pipeline_name_field};
use crate::domain::pipeline::parse_pipeline;

/// What a rename did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameOutcome {
    /// The name was already the pipeline's: nothing moved.
    Unchanged { id: String, name: String },
    /// Display-only (same stem, e.g. "foo-bar" → "foo bar"): the `name:` line
    /// was spliced in place.
    InPlace { id: String, name: String },
    /// The file and its prompts moved to the slug of the new name.
    Moved {
        id: String,
        name: String,
        path: PathBuf,
    },
}

impl Pipelines {
    /// Refused when the target stem or the target name is taken, and while a
    /// Run not yet completed uses the pipeline.
    pub async fn rename(
        &self,
        actor: &Actor,
        id: &str,
        name: &str,
    ) -> Result<RenameOutcome, PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Pipeline, Verb::Update).on(id),
        )?;
        let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
        let new_name = name.trim().to_string();
        if new_name.is_empty() {
            return Err(PipelineError::NameRequired);
        }
        let parsed = parse_pipeline(&yaml)?;
        let old_name = parsed.pipeline.name.trim().to_string();
        if old_name == new_name {
            return Ok(RenameOutcome::Unchanged {
                id: id.to_string(),
                name: new_name,
            });
        }
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        let new_id = pipeline_slug(&new_name);
        if new_id == id {
            let new_yaml =
                rewrite_pipeline_name_field(&yaml, &new_name).ok_or(PipelineError::NoNameLine)?;
            self.registry
                .write(id, &new_yaml)
                .map_err(PipelineError::Storage)?;
            let path = self.registry.path_of(id);
            self.events.changed(id, &path.to_string_lossy());
            info!("Pipeline {id} display name updated to {new_name}");
            return Ok(RenameOutcome::InPlace {
                id: id.to_string(),
                name: new_name,
            });
        }
        self.refuse_a_taken_identity(id, &new_name, &new_id).await?;
        let new_yaml =
            rewrite_pipeline_name_field(&yaml, &new_name).ok_or(PipelineError::NoNameLine)?;
        let new_path = self
            .registry
            .relocate(id, &new_id, &new_yaml)
            .map_err(PipelineError::Storage)?;
        self.renames
            .record(&Renamed {
                old_id: id.to_string(),
                old_name,
                new_id: new_id.clone(),
                new_name: new_name.clone(),
            })
            .await;
        // Announce the move ourselves: self-writes are suppressed by the
        // watcher, and clients must refresh the list under the NEW id at once.
        self.events.changed(&new_id, &new_path.to_string_lossy());
        info!("Pipeline {id} renamed to {new_id}");
        Ok(RenameOutcome::Moved {
            id: new_id,
            name: new_name,
            path: new_path,
        })
    }

    /// The refusals of a move to `new_id` / `new_name`: another file owns the
    /// stem, another pipeline carries the name, or a Run not yet completed
    /// uses the pipeline (its completion path resolves it by id).
    pub(crate) async fn refuse_a_taken_identity(
        &self,
        id: &str,
        new_name: &str,
        new_id: &str,
    ) -> Result<(), PipelineError> {
        if let Some(conflict) = self.name_conflict(id, new_name, new_id) {
            return Err(PipelineError::NameTaken(conflict));
        }
        let active = self.runs.using(id).await;
        if active > 0 {
            return Err(PipelineError::ActiveRuns(active));
        }
        Ok(())
    }

    /// The two collisions a visible-name change must refuse, either of which
    /// would list two pipelines under one name.
    fn name_conflict(&self, self_id: &str, new_name: &str, new_id: &str) -> Option<String> {
        if new_id != self_id && self.registry.exists(new_id) {
            return Some(format!("a pipeline file '{new_id}.yaml' already exists"));
        }
        self.registry
            .files()
            .iter()
            .any(|file| file.id != self_id && listed_name(file).trim() == new_name)
            .then(|| format!("a pipeline already uses the name \"{new_name}\""))
    }
}
