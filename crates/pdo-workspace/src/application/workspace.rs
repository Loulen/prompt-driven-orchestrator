//! The workspace context as the rest of PDO sees it: its ports, wired once by
//! the composition root, and the extension host every mutation asks first.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::repository_path::member_spellings;
use pdo_kernel::{Actor, Operation, ResourceKind};

use super::ports::{
    FileBrowser, Git, ProjectEvents, ProjectRepository, ProvisioningRulesRepository,
    RecentRepositories, SourceTree,
};

/// One adapter per port.
pub struct WorkspacePorts {
    pub projects: Arc<dyn ProjectRepository>,
    pub provisioning: Arc<dyn ProvisioningRulesRepository>,
    pub recent: Arc<dyn RecentRepositories>,
    pub events: Arc<dyn ProjectEvents>,
    pub git: Arc<dyn Git>,
    pub source_tree: Arc<dyn SourceTree>,
    pub files: Arc<dyn FileBrowser>,
}

/// The workspace context: every use case is a method.
pub struct Workspace {
    pub(crate) projects: Arc<dyn ProjectRepository>,
    pub(crate) provisioning: Arc<dyn ProvisioningRulesRepository>,
    pub(crate) recent: Arc<dyn RecentRepositories>,
    pub(crate) events: Arc<dyn ProjectEvents>,
    pub(crate) git: Arc<dyn Git>,
    pub(crate) source_tree: Arc<dyn SourceTree>,
    pub(crate) files: Arc<dyn FileBrowser>,
    host: Arc<dyn ExtensionHost>,
}

impl Workspace {
    pub fn new(ports: WorkspacePorts, host: Arc<dyn ExtensionHost>) -> Self {
        Self {
            projects: ports.projects,
            provisioning: ports.provisioning,
            recent: ports.recent,
            events: ports.events,
            git: ports.git,
            source_tree: ports.source_tree,
            files: ports.files,
            host,
        }
    }

    /// The extension host: the attributes of the Projets.
    pub(crate) fn host(&self) -> &dyn ExtensionHost {
        self.host.as_ref()
    }

    /// The Autorisation d'opération: every mutation goes through here first.
    /// An Operation on a Projet is in that Projet (CORE-3).
    pub(crate) fn authorize(&self, actor: &Actor, operation: Operation) -> Result<(), Denied> {
        let operation = match (&operation.resource, operation.target.clone()) {
            (ResourceKind::Project, Some(id)) if operation.is_outside_projects() => {
                operation.in_projects([id])
            }
            _ => operation,
        };
        authorize(self.host.as_ref(), actor, &operation)
    }

    /// The Projets the repository `path` is in: the one that owns it, or none
    /// (also when the Projets cannot be read). Its other spellings are tried
    /// when it is no member, as the Run context places a Run
    /// ([`member_spellings`]).
    pub(crate) async fn projects_of_repository(&self, path: &str) -> Vec<String> {
        for spelling in member_spellings(path) {
            match self.projects.owner_of(&spelling).await {
                Ok(Some(owner)) => return vec![owner.id],
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("the Projet owning {path} cannot be read: {e}");
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }
}
