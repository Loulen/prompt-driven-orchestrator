//! What the pipeline context answers without changing anything.

use std::collections::HashMap;
use std::path::PathBuf;

use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::SidecarSkill;
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::SkillBank;
use super::registry_names::listed_name;
use crate::domain::library::LibraryEntry;
use crate::domain::pipeline::{parse_pipeline, PipelineDef, VariableDef};
use crate::domain::portable;

/// One row of the pipeline list. A file that does not parse is still listed,
/// under its stem, with no node and a required prompt.
#[derive(Debug, Clone)]
pub struct PipelineSummary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub node_count: usize,
    pub modified: Option<String>,
    pub variables: HashMap<String, VariableDef>,
    pub prompt_required: bool,
    /// Why the file does not parse, when it does not: the list marks it
    /// invalid.
    pub invalid: Option<String>,
}

/// One pipeline, parsed, with its prompts and the parse's warnings.
#[derive(Debug, Clone)]
pub struct PipelineDetail {
    pub id: String,
    pub path: PathBuf,
    pub yaml: String,
    pub pipeline: PipelineDef,
    pub prompts: HashMap<String, String>,
    pub diagnostics: Vec<String>,
}

/// The skills sidecar of a pipeline's portable document.
#[derive(Debug, Clone)]
pub struct SidecarExport {
    pub pipeline_name: String,
    pub skills: Vec<SidecarSkill>,
}

impl Pipelines {
    /// The instance's pipelines, by name, case-insensitively. Folds the
    /// retired registries in first (once).
    pub fn list(&self) -> Result<Vec<PipelineSummary>, PipelineError> {
        self.registry
            .unify_legacy()
            .map_err(PipelineError::Storage)?;
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        let mut pipelines: Vec<PipelineSummary> = self
            .registry
            .files()
            .into_iter()
            .map(|file| {
                let parsed = file.yaml.as_deref().map(parse_pipeline);
                let invalid = match &parsed {
                    None => Some("the file cannot be read".to_string()),
                    Some(Err(e)) => Some(e.to_string()),
                    Some(Ok(_)) => None,
                };
                let (name, node_count, variables, prompt_required) =
                    match parsed.and_then(Result::ok) {
                        Some(r) => (
                            r.pipeline.name.clone(),
                            r.pipeline.nodes.len(),
                            r.pipeline.variables.clone(),
                            r.pipeline.prompt_required,
                        ),
                        None => (listed_name(&file), 0, HashMap::new(), true),
                    };
                PipelineSummary {
                    id: file.id,
                    name,
                    path: file.path,
                    node_count,
                    modified: file.modified,
                    variables,
                    prompt_required,
                    invalid,
                }
            })
            .collect();
        pipelines.sort_by_key(|entry| entry.name.to_lowercase());
        Ok(pipelines)
    }

    /// Whether the registry holds the pipeline `id`.
    pub fn exists(&self, id: &str) -> bool {
        self.registry.exists(id)
    }

    /// The pipeline `id`, parsed, with the prompts of its nodes.
    pub fn detail(&self, id: &str) -> Result<PipelineDetail, PipelineError> {
        let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
        let parsed = parse_pipeline(&yaml)?;
        let on_disk = self.registry.prompts(id);
        let prompts = parsed
            .pipeline
            .nodes
            .iter()
            .filter_map(|node| {
                on_disk
                    .get(&node.id)
                    .map(|content| (node.id.clone(), content.clone()))
            })
            .collect();
        Ok(PipelineDetail {
            id: id.to_string(),
            path: self.registry.path_of(id),
            yaml,
            pipeline: parsed.pipeline,
            prompts,
            diagnostics: parsed.diagnostics.into_iter().map(|d| d.message).collect(),
        })
    }

    /// The pipeline `id`, parsed, as a reader that only wants the document.
    pub(crate) fn parsed(&self, id: &str) -> Result<PipelineDef, PipelineError> {
        let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
        Ok(parse_pipeline(&yaml)?.pipeline)
    }

    /// The portable document of `id` (ADR-0059): its YAML and its prompts in
    /// one file, harness selections stripped.
    pub fn export_document(&self, id: &str) -> Result<String, PipelineError> {
        let pipeline = self.parsed(id)?;
        portable::export(&pipeline, &self.registry.prompts(id)).map_err(PipelineError::Storage)
    }

    /// The skills sidecar of `id`'s portable document (#673); `None` when it
    /// references no skill.
    pub fn document_skills(&self, id: &str) -> Result<Option<SidecarExport>, PipelineError> {
        let pipeline = self.parsed(id)?;
        Ok(self.sidecar_of(&pipeline))
    }

    /// The skills sidecar of a parsed pipeline (see [`sidecar_export`]).
    pub fn sidecar_of(&self, pipeline: &PipelineDef) -> Option<SidecarExport> {
        sidecar_export(self.skills.as_ref(), pipeline)
    }

    /// The node library, by name.
    pub fn library(&self) -> Vec<LibraryEntry> {
        self.library.list()
    }

    /// The library entry `name`, to drop onto a canvas.
    pub fn library_entry(&self, name: &str) -> Option<LibraryEntry> {
        self.library.get(name)
    }
}

/// The unique skill references of a pipeline's nodes, in first-seen order.
pub(crate) fn skill_references(pipeline: &PipelineDef) -> Vec<SkillRef> {
    pdo_skill::domain::sidecar::referenced_skills(
        pipeline.nodes.iter().map(|node| node.skills.as_slice()),
    )
}

/// The skills sidecar of a parsed pipeline, read from `bank`; `None` when the
/// pipeline references no skill. A referenced id missing from the bank is left
/// out (and logged): the importing instance will warn about it. The Runs'
/// snapshot export reads it too.
pub fn sidecar_export(bank: &dyn SkillBank, pipeline: &PipelineDef) -> Option<SidecarExport> {
    let references = skill_references(pipeline);
    if references.is_empty() {
        return None;
    }
    let (skills, absent) = bank.sidecar_of(&references);
    if !absent.is_empty() {
        info!(
            "skills sidecar of '{}': {} referenced skill(s) not in the bank: {}",
            pipeline.name,
            absent.len(),
            absent.join(", ")
        );
    }
    Some(SidecarExport {
        pipeline_name: pipeline.name.clone(),
        skills,
    })
}
