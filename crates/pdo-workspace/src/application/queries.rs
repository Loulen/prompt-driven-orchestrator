//! What the workspace answers without changing anything. The reads guarded by
//! the Autorisation d'opération (the Projets, the disk, the recent
//! repositories) are in
//! [`read_access`](super::read_access).

use std::path::{Path, PathBuf};

use pdo_kernel::attributes::{self, AttributeFilter, AttributedList};
use pdo_kernel::ResourceKind;

use crate::domain::branches::BranchList;
use crate::domain::browse::BrowseRootError;
use crate::domain::project::Project;
use crate::domain::provisioning::{
    preview_levels, ProvisioningPlan, ProvisioningRules, ProvisioningScope, ScopedRules,
};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::{Listing, StorageError, StoreResult};
use super::save_provisioning_rules::INSTANCE_SCOPE_ID;
use super::workspace::Workspace;

/// How many repositories the recent list holds.
pub const RECENT_REPOSITORIES: usize = 5;

/// A provisioning preview request.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub repository: String,
    pub git_ref: String,
    pub scope: ProvisioningScope,
    /// The rules being edited at `scope`.
    pub rules: ProvisioningRules,
    /// The inherited levels the editor already holds; `None` reads them from
    /// the stores, for the Projet `project_id`.
    pub inherited: Option<Vec<ScopedRules>>,
    /// The Projet the Run is launched in (ADR-0087); `None` for none.
    pub project_id: Option<String>,
}

impl Workspace {
    pub async fn list_projects(&self) -> StoreResult<Vec<Project>> {
        self.projects.list().await
    }

    pub async fn project(&self, id: &str) -> StoreResult<Option<Project>> {
        self.projects.get(id).await
    }

    /// Every Projet carrying the attributes `filter` asks for, each with its
    /// attributes: the list the operator reads.
    pub async fn projects_carrying(&self, filter: AttributeFilter) -> StoreResult<Vec<Project>> {
        let projects = self.projects.list().await?;
        let attributed = AttributedList::read(self.host(), ResourceKind::Project, filter).await;
        Ok(projects
            .into_iter()
            .filter(|project| attributed.keeps(&project.id))
            .map(|mut project| {
                project.attributes = attributed.of(&project.id);
                project
            })
            .collect())
    }

    /// One Projet, with its attributes.
    pub async fn attributed_project(&self, id: &str) -> StoreResult<Option<Project>> {
        Ok(match self.projects.get(id).await? {
            Some(project) => Some(self.with_attributes(project).await),
            None => None,
        })
    }

    /// `project`, with its attributes.
    pub async fn with_attributes(&self, mut project: Project) -> Project {
        project.attributes =
            attributes::of_entity(self.host(), ResourceKind::Project, &project.id).await;
        project
    }

    pub async fn provisioning_rules(
        &self,
        scope: ProvisioningScope,
        scope_id: &str,
    ) -> StoreResult<ProvisioningRules> {
        self.provisioning.load(scope, scope_id).await
    }

    /// Every provisioning level a Run launched in `project` inherits, coarsest
    /// first: the instance, that Projet (if any, ADR-0087), the Run's own
    /// rules, then the isolated node's when it states some.
    pub async fn scoped_provisioning_rules(
        &self,
        project: Option<&str>,
        run: ProvisioningRules,
        node: ProvisioningRules,
    ) -> StoreResult<Vec<ScopedRules>> {
        let mut rules = vec![ScopedRules {
            scope: ProvisioningScope::Instance,
            rules: self
                .provisioning
                .load(ProvisioningScope::Instance, INSTANCE_SCOPE_ID)
                .await?,
        }];
        if let Some(project) = project {
            rules.push(ScopedRules {
                scope: ProvisioningScope::Project,
                rules: self
                    .provisioning
                    .load(ProvisioningScope::Project, project)
                    .await?,
            });
        }
        rules.push(ScopedRules {
            scope: ProvisioningScope::Run,
            rules: run,
        });
        if !node.is_empty() {
            rules.push(ScopedRules {
                scope: ProvisioningScope::IsolatedNode,
                rules: node,
            });
        }
        Ok(rules)
    }

    /// What the rules being edited would provision, resolved over the source
    /// repository at `git_ref`.
    pub async fn preview_provisioning(
        &self,
        request: PreviewRequest,
    ) -> WorkspaceResult<ProvisioningPlan> {
        let repository = PathBuf::from(request.repository.trim());
        if !repository.is_absolute() {
            return Err(WorkspaceError::Invalid(
                "repository must be an absolute path".into(),
            ));
        }
        let inherited = match request.inherited {
            Some(rules) => rules,
            None => self
                .scoped_provisioning_rules(
                    request.project_id.as_deref(),
                    ProvisioningRules::default(),
                    ProvisioningRules::default(),
                )
                .await
                .map_err(|e| StorageError(format!("failed to load provisioning rules: {e}")))?,
        };
        let levels = preview_levels(inherited, request.scope, request.rules);
        self.source_tree
            .plan(&repository, &levels, &request.git_ref)
            .map_err(|e| WorkspaceError::Invalid(format!("{e:#}")))
    }

    /// `path` as a repository a Run can work in.
    pub fn validate_repository(&self, path: &str) -> Result<PathBuf, String> {
        self.git.validate_repository(path)
    }

    /// The branches of the repository at `path`, read locally.
    pub fn branches(&self, path: &str) -> WorkspaceResult<BranchList> {
        let repo = self
            .git
            .validate_repository(path)
            .map_err(WorkspaceError::Invalid)?;
        self.git.branches(&repo).map_err(WorkspaceError::Git)
    }

    /// The branches of an already validated repository.
    pub fn branches_of(&self, repo: &Path) -> Result<BranchList, String> {
        self.git.branches(repo)
    }

    /// One level of the directory at `path` (the default chain when `None`).
    pub fn browse(
        &self,
        path: Option<&str>,
        files: bool,
        hidden: bool,
    ) -> Result<Listing, BrowseRootError> {
        self.files.browse(path, files, hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::fixture;

    fn rules(copy: &str) -> ProvisioningRules {
        ProvisioningRules {
            copy: vec![copy.into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn inherited_levels_follow_the_projet_the_run_is_launched_in() {
        let f = fixture();
        f.provisioning
            .put(ProvisioningScope::Project, "p1", rules("project"));
        let levels = f
            .workspace
            .scoped_provisioning_rules(Some("p1"), rules("run"), ProvisioningRules::default())
            .await
            .unwrap();
        let scopes: Vec<_> = levels.iter().map(|l| l.scope).collect();
        assert_eq!(
            scopes,
            vec![
                ProvisioningScope::Instance,
                ProvisioningScope::Project,
                ProvisioningScope::Run
            ]
        );
        let elsewhere = f
            .workspace
            .scoped_provisioning_rules(None, rules("run"), rules("node"))
            .await
            .unwrap();
        let scopes: Vec<_> = elsewhere.iter().map(|l| l.scope).collect();
        assert_eq!(
            scopes,
            vec![
                ProvisioningScope::Instance,
                ProvisioningScope::Run,
                ProvisioningScope::IsolatedNode
            ]
        );
    }

    #[tokio::test]
    async fn a_preview_resolves_the_edited_rules_over_the_inherited_ones() {
        let f = fixture();
        f.provisioning.put(
            ProvisioningScope::Instance,
            INSTANCE_SCOPE_ID,
            rules(".env"),
        );
        f.workspace
            .preview_provisioning(PreviewRequest {
                repository: " /repo ".into(),
                git_ref: "HEAD".into(),
                scope: ProvisioningScope::Project,
                rules: rules("edited"),
                inherited: None,
                project_id: None,
            })
            .await
            .unwrap();
        let (repo, levels, git_ref) = f.source_tree.last();
        assert_eq!(repo, PathBuf::from("/repo"));
        assert_eq!(git_ref, "HEAD");
        assert_eq!(
            levels,
            vec![
                ScopedRules {
                    scope: ProvisioningScope::Instance,
                    rules: rules(".env")
                },
                ScopedRules {
                    scope: ProvisioningScope::Project,
                    rules: rules("edited")
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_relative_repository_cannot_be_previewed() {
        let f = fixture();
        let err = f
            .workspace
            .preview_provisioning(PreviewRequest {
                repository: "repo".into(),
                git_ref: "HEAD".into(),
                scope: ProvisioningScope::Run,
                rules: ProvisioningRules::default(),
                inherited: Some(vec![]),
                project_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            err,
            WorkspaceError::Invalid("repository must be an absolute path".into())
        );
    }
}
