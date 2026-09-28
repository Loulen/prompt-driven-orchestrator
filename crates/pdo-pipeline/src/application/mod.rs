//! The pipeline use cases. Every mutation an actor asks for builds an
//! Opération and asks the Autorisation d'opération before touching anything;
//! reads, and the unification of the retired registries (a transition of the
//! daemon, D10), go straight to the ports.

mod attributes;
mod create_pipeline;
mod delete_pipeline;
mod duplicate_pipeline;
mod error;
mod import_document;
mod import_workflow;
mod library;
mod pipelines;
pub mod ports;
mod queries;
mod registry_names;
mod rename_pipeline;
mod save_pipeline;

pub use error::PipelineError;
pub use import_document::ImportedDocument;
pub use import_workflow::{ImportedWorkflow, WorkflowImportError};
pub use pipelines::{Pipelines, PipelinesPorts};
pub use queries::{sidecar_export, PipelineDetail, PipelineSummary, SidecarExport};
pub use rename_pipeline::RenameOutcome;
pub use save_pipeline::SaveOutcome;

/// Where a new or moved pipeline landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub id: String,
    pub path: std::path::PathBuf,
}
