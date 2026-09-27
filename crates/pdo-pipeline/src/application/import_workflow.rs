//! Import a Claude Code workflow `.js` as a draft pipeline (#155 / ADR-0016).

use std::path::PathBuf;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::registry_names::lenient_names;
use crate::domain::naming::available_identity;
use crate::domain::pipeline::parse_pipeline;
use crate::domain::workflow_import::import_workflow_js;

/// Where an imported workflow landed, and what the translation lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedWorkflow {
    pub id: String,
    pub path: PathBuf,
    pub warnings: Vec<String>,
}

/// Why a workflow did not import.
#[derive(Debug)]
pub enum WorkflowImportError {
    Pipeline(PipelineError),
    /// The source is outside the subset the importer reads, or the draft does
    /// not parse: the words of the failure.
    Untranslatable(String),
    /// The draft could not be written.
    Publish(String),
}

impl From<PipelineError> for WorkflowImportError {
    fn from(error: PipelineError) -> Self {
        Self::Pipeline(error)
    }
}

impl Pipelines {
    /// The `.js` arrives over the wire and is parsed to an AST, never
    /// executed; `filename` only names the fallback. Lossy translations ride
    /// back as warnings.
    pub fn import_workflow(
        &self,
        actor: &Actor,
        filename: &str,
        content: &str,
    ) -> Result<ImportedWorkflow, WorkflowImportError> {
        self.authorize(actor, Operation::new(ResourceKind::Pipeline, Verb::Create))
            .map_err(PipelineError::Denied)?;
        let stem = std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("imported-workflow");
        let result = import_workflow_js(content, stem)
            .map_err(|e| WorkflowImportError::Untranslatable(e.to_string()))?;
        let parsed = parse_pipeline(&result.yaml_text)
            .map_err(|e| WorkflowImportError::Untranslatable(e.to_string()))?
            .pipeline;
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry.into());
        }
        let files = self.registry.files();
        let (id, name) = available_identity(&result.name, &lenient_names(&files), |stem| {
            self.registry.exists(stem)
        });
        let mut imported = parsed;
        imported.name = name;
        let path = self
            .registry
            .publish(&id, &imported, &result.prompts)
            .map_err(WorkflowImportError::Publish)?;
        Ok(ImportedWorkflow {
            id,
            path,
            warnings: result.warnings.iter().map(|d| d.message.clone()).collect(),
        })
    }
}
