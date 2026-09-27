//! The skill context as the rest of PDO sees it: its ports, wired once by the
//! composition root, and the extension host every mutation asks first.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};

use super::ports::{
    FolderIndex, SkillContent, SkillDelivery, SkillIndex, SkillReferents, SourceHistory,
    SourceScanner,
};

/// One adapter per port.
pub struct SkillsPorts {
    pub skills: Arc<dyn SkillIndex>,
    pub folders: Arc<dyn FolderIndex>,
    pub sources: Arc<dyn SourceHistory>,
    pub content: Arc<dyn SkillContent>,
    pub scanner: Arc<dyn SourceScanner>,
    pub delivery: Arc<dyn SkillDelivery>,
    pub referents: Arc<dyn SkillReferents>,
}

/// The skill context: every use case is a method.
pub struct Skills {
    pub(crate) skills: Arc<dyn SkillIndex>,
    pub(crate) folders: Arc<dyn FolderIndex>,
    pub(crate) sources: Arc<dyn SourceHistory>,
    pub(crate) content: Arc<dyn SkillContent>,
    pub(crate) scanner: Arc<dyn SourceScanner>,
    pub(crate) delivery: Arc<dyn SkillDelivery>,
    pub(crate) referents: Arc<dyn SkillReferents>,
    host: Arc<dyn ExtensionHost>,
}

impl Skills {
    pub fn new(ports: SkillsPorts, host: Arc<dyn ExtensionHost>) -> Self {
        Self {
            skills: ports.skills,
            folders: ports.folders,
            sources: ports.sources,
            content: ports.content,
            scanner: ports.scanner,
            delivery: ports.delivery,
            referents: ports.referents,
            host,
        }
    }

    /// The Autorisation d'opération: every mutation goes through here first.
    pub(crate) fn authorize(&self, actor: &Actor, operation: Operation) -> Result<(), Denied> {
        authorize(self.host.as_ref(), actor, &operation)
    }
}
