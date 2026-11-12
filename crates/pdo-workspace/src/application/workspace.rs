//! The workspace context as the rest of PDO sees it: its ports, wired once by
//! the composition root, and the extension host every mutation asks first.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::repository_path::member_spellings;
use pdo_kernel::{Actor, Operation, ResourceKind};

use super::clone_jobs::CloneJobs;
use super::ports::{
    ClonesFolder, FileBrowser, Git, ProjectDeletions, ProjectEvents, ProjectRepository,
    ProvisioningRulesRepository, RecentRepositories, SkillSelections, SourceTree, StoreResult,
};
use crate::domain::project::Project;

/// One adapter per port.
pub struct WorkspacePorts {
    pub projects: Arc<dyn ProjectRepository>,
    pub provisioning: Arc<dyn ProvisioningRulesRepository>,
    pub recent: Arc<dyn RecentRepositories>,
    pub events: Arc<dyn ProjectEvents>,
    pub deletions: Arc<dyn ProjectDeletions>,
    pub git: Arc<dyn Git>,
    pub source_tree: Arc<dyn SourceTree>,
    pub files: Arc<dyn FileBrowser>,
    pub clones_folder: Arc<dyn ClonesFolder>,
    /// Whether a skills selection may replace a Projet's (the skill context).
    pub skills: Arc<dyn SkillSelections>,
}

/// The workspace context: every use case is a method.
pub struct Workspace {
    pub(crate) projects: Arc<dyn ProjectRepository>,
    pub(crate) provisioning: Arc<dyn ProvisioningRulesRepository>,
    pub(crate) recent: Arc<dyn RecentRepositories>,
    pub(crate) events: Arc<dyn ProjectEvents>,
    pub(crate) deletions: Arc<dyn ProjectDeletions>,
    pub(crate) git: Arc<dyn Git>,
    pub(crate) source_tree: Arc<dyn SourceTree>,
    pub(crate) files: Arc<dyn FileBrowser>,
    pub(crate) clones_folder: Arc<dyn ClonesFolder>,
    pub(crate) skills: Arc<dyn SkillSelections>,
    /// The clones running in the background, and the last ones finished.
    pub(crate) clones: Arc<CloneJobs>,
    host: Arc<dyn ExtensionHost>,
}

impl Workspace {
    pub fn new(ports: WorkspacePorts, host: Arc<dyn ExtensionHost>) -> Self {
        Self {
            projects: ports.projects,
            provisioning: ports.provisioning,
            recent: ports.recent,
            events: ports.events,
            deletions: ports.deletions,
            git: ports.git,
            source_tree: ports.source_tree,
            files: ports.files,
            clones_folder: ports.clones_folder,
            skills: ports.skills,
            clones: Arc::new(CloneJobs::default()),
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

    /// The Projets the repository `path` is in: every Projet it is a member
    /// of (ADR-0087), or none (also when the Projets cannot be read). Its
    /// other spellings are tried when it is no member ([`member_spellings`]):
    /// `/repo/` and `/repo/.` are in the Projets of `/repo`.
    pub(crate) async fn projects_of_repository(&self, path: &str) -> Vec<String> {
        match self.projects_holding(path).await {
            Ok(projects) => projects.into_iter().map(|p| p.id).collect(),
            Err(e) => {
                tracing::warn!("the Projets of {path} cannot be read: {e}");
                Vec::new()
            }
        }
    }

    /// The Projets holding the repository `path`, in the order it joined
    /// them: those it is a member of under its first spelling that is one
    /// ([`member_spellings`]), none when it is no member.
    pub async fn projects_holding(&self, path: &str) -> StoreResult<Vec<Project>> {
        for spelling in member_spellings(path) {
            let projects = self.projects.containing(&spelling).await?;
            if !projects.is_empty() {
                return Ok(projects);
            }
        }
        Ok(Vec::new())
    }
}
