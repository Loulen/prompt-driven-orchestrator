//! The pipeline context as the rest of PDO sees it: its ports, wired once by
//! the composition root, and the extension host every mutation asks first.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};

use super::drafts::Drafts;
use super::ports::{
    ActiveRuns, DraftDocuments, DraftStore, KnownProjects, NodeLibrary, PipelineEvents,
    PipelineRegistry, PipelineValidations, RenameLedger, SkillBank,
};

/// One adapter per port.
pub struct PipelinesPorts {
    pub registry: Arc<dyn PipelineRegistry>,
    pub library: Arc<dyn NodeLibrary>,
    pub events: Arc<dyn PipelineEvents>,
    pub runs: Arc<dyn ActiveRuns>,
    pub renames: Arc<dyn RenameLedger>,
    pub skills: Arc<dyn SkillBank>,
    pub validations: Arc<dyn PipelineValidations>,
    pub projects: Arc<dyn KnownProjects>,
    /// Where the shared drafts are kept (ADR-0088).
    pub drafts: Arc<dyn DraftStore>,
    /// The replicated documents of the drafts.
    pub documents: Arc<dyn DraftDocuments>,
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The pipeline context: every use case is a method.
pub struct Pipelines {
    pub(crate) registry: Arc<dyn PipelineRegistry>,
    pub(crate) library: Arc<dyn NodeLibrary>,
    pub(crate) events: Arc<dyn PipelineEvents>,
    pub(crate) runs: Arc<dyn ActiveRuns>,
    pub(crate) renames: Arc<dyn RenameLedger>,
    pub(crate) skills: Arc<dyn SkillBank>,
    pub(crate) validations: Arc<dyn PipelineValidations>,
    pub(crate) projects: Arc<dyn KnownProjects>,
    pub(crate) drafts: Drafts,
    host: Arc<dyn ExtensionHost>,
}

impl Pipelines {
    pub fn new(ports: PipelinesPorts) -> Self {
        Self {
            registry: ports.registry,
            library: ports.library,
            events: ports.events,
            runs: ports.runs,
            renames: ports.renames,
            skills: ports.skills,
            validations: ports.validations,
            projects: ports.projects,
            drafts: Drafts::new(ports.drafts, ports.documents),
            host: ports.extensions,
        }
    }

    /// The extension host: the attributes of the pipelines.
    pub(crate) fn host(&self) -> &dyn ExtensionHost {
        self.host.as_ref()
    }

    /// The Autorisation d'opération: every mutation goes through here first.
    pub(crate) fn authorize(&self, actor: &Actor, operation: Operation) -> Result<(), Denied> {
        authorize(self.host.as_ref(), actor, &operation)
    }
}
