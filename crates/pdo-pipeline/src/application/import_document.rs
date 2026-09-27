//! Import a portable pipeline document (ADR-0059), with its skills sidecar
//! (#673).

use std::path::PathBuf;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_skill::domain::sidecar::{SidecarImportReport, SidecarSkill};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::queries::skill_references;
use super::registry_names::lenient_names;
use crate::domain::naming::available_identity;
use crate::domain::portable;

/// Where an imported document landed, what it warned about, what the bank did.
#[derive(Debug, Clone)]
pub struct ImportedDocument {
    pub id: String,
    pub path: PathBuf,
    pub warnings: Vec<String>,
    pub skills: SidecarImportReport,
}

impl Pipelines {
    /// The bank first, the YAML second: a created skill is referenced by the
    /// id the document carries, so the pipeline is launchable as soon as it is
    /// written, and its node references are relabelled with the bank's names.
    /// The import folder is named after the document's own name (its
    /// provenance), before the "(2)" a same-named pipeline may add.
    pub async fn import_document(
        &self,
        actor: &Actor,
        document: &str,
        sidecar: &[SidecarSkill],
    ) -> Result<ImportedDocument, PipelineError> {
        self.authorize(actor, Operation::new(ResourceKind::Pipeline, Verb::Create))?;
        let interpreted = portable::interpret(document).map_err(PipelineError::BadRequest)?;
        self.import_interpreted(actor, interpreted, sidecar).await
    }

    async fn import_interpreted(
        &self,
        actor: &Actor,
        interpreted: portable::InterpretedDocument,
        sidecar: &[SidecarSkill],
    ) -> Result<ImportedDocument, PipelineError> {
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        let files = self.registry.files();
        let (id, name) =
            available_identity(&interpreted.pipeline.name, &lenient_names(&files), |stem| {
                self.registry.exists(stem)
            });
        let mut imported = interpreted.pipeline;
        let mut warnings = interpreted.warnings;
        let skills = self
            .skills
            .import_sidecar(actor, &imported.name, skill_references(&imported), sidecar)
            .await
            .map_err(PipelineError::Skills)?;
        let final_names = skills.final_names();
        for node in &mut imported.nodes {
            for skill in &mut node.skills {
                if let Some(name) = final_names.get(&skill.id) {
                    skill.name = name.clone();
                }
            }
        }
        warnings.extend(skills.warnings.iter().cloned());
        imported.name = name;
        let path = self
            .registry
            .publish(&id, &imported, &interpreted.prompts)
            .map_err(PipelineError::Storage)?;
        Ok(ImportedDocument {
            id,
            path,
            warnings,
            skills,
        })
    }
}
