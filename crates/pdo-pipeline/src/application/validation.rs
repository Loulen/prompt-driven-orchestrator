//! The validation of the pipelines (CONTEXT.md « Pipeline validé »): which
//! Projets the launch forms offer each pipeline to. Data of the instance, on
//! the model of the attributes: set when a pipeline is born (a test pipeline),
//! carried by a rename, forgotten with the pipeline, never exported.

use std::collections::HashMap;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{error, warn};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use crate::domain::validation::PipelineValidation;

/// The verb of the Opération that validates a pipeline (`pipeline` ×
/// `validate`).
pub const VALIDATE: &str = "validate";

impl Pipelines {
    /// The validation of every pipeline that states one; a pipeline absent
    /// from the map is validated for every Projet. A store that cannot answer
    /// is logged and reads as « every Projet »: the launch forms then offer
    /// everything, as before validations existed.
    pub async fn validations(&self) -> HashMap<String, PipelineValidation> {
        self.validations.all().await.unwrap_or_else(|e| {
            error!("the validations of the pipelines cannot be read: {e}");
            HashMap::new()
        })
    }

    /// Replace the validation of the pipeline `id`, as `actor` asks. Every
    /// Projet named must exist; nothing is written otherwise. Returns what is
    /// stored (an empty Projet list is a test pipeline).
    pub async fn validate(
        &self,
        actor: &Actor,
        id: &str,
        validation: PipelineValidation,
    ) -> Result<PipelineValidation, PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Pipeline, Verb::Action(VALIDATE.into())).on(id),
        )?;
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        let validation = validation.normalized();
        for project_id in validation.project_ids() {
            match self.projects.exists(project_id).await {
                Ok(true) => {}
                Ok(false) => return Err(PipelineError::UnknownProject(project_id.clone())),
                Err(e) => return Err(PipelineError::Storage(e)),
            }
        }
        self.validations
            .put(id, &validation)
            .await
            .map_err(PipelineError::Storage)?;
        self.events.validation_changed(id);
        Ok(validation)
    }

    /// The pipeline `id` was just created, duplicated or imported: it is born
    /// a test pipeline. Best effort, like the Stats of a rename: the file is
    /// written, and failing the request over a lost row would lie about it.
    pub(crate) async fn born_unvalidated(&self, id: &str) {
        if let Err(e) = self.validations.put(id, &PipelineValidation::None).await {
            warn!("failed to mark pipeline {id} as a test pipeline: {e}");
        }
    }

    /// A rename moved the pipeline `from` to `to`: its validation follows.
    pub(crate) async fn carry_validation(&self, from: &str, to: &str) {
        if from == to {
            return;
        }
        if let Err(e) = self.validations.carry(from, to).await {
            warn!("failed to carry the validation of pipeline {from} to {to}: {e}");
        }
    }

    /// The pipeline `id` is deleted: its validation goes with it.
    pub(crate) async fn forget_validation(&self, id: &str) {
        if let Err(e) = self.validations.forget(id).await {
            warn!("failed to forget the validation of pipeline {id}: {e}");
        }
    }

    /// The Projet `project_id` is deleted: it is withdrawn from every
    /// validation that names it, and a pipeline left with no Projet becomes a
    /// test pipeline. Each pipeline changed is announced.
    pub async fn forget_project(&self, project_id: &str) {
        let stated = match self.validations.all().await {
            Ok(stated) => stated,
            Err(e) => {
                error!("the validations cannot be read to withdraw Projet {project_id}: {e}");
                return;
            }
        };
        let mut changed: Vec<(String, PipelineValidation)> = stated
            .into_iter()
            .filter_map(|(id, validation)| {
                validation
                    .without_project(project_id)
                    .map(|kept| (id, kept))
            })
            .collect();
        changed.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, kept) in changed {
            match self.validations.put(&id, &kept).await {
                Ok(()) => self.events.validation_changed(&id),
                Err(e) => error!("failed to withdraw Projet {project_id} from pipeline {id}: {e}"),
            }
        }
    }
}
