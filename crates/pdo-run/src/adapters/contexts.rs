//! The run context's ports over the upstream contexts it reads: the declared
//! wait of the session context, the Projets of the workspace context, the bank
//! and the delivery of the skill context.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use pdo_session::Session;
use pdo_skill::domain::delivery::SkippedSkill;
use pdo_skill::domain::selection::ActiveSkill;
use pdo_skill::Skills;
use pdo_workspace::application::Workspace;
use pdo_workspace::domain::project::Project;
use pdo_workspace::domain::provisioning::{ProvisioningRules, ScopedRules};

use crate::application::ports::{BoxFuture, DeclaredWaits, NodeSkills, Projects};

/// The declared wait, over the session context.
pub struct SessionWaits(pub Arc<Session>);

impl DeclaredWaits for SessionWaits {
    fn on_completion_not_released<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0
                .declare_wait()
                .on_completion_not_released(run_id, node_id, iter)
                .await
        })
    }
}

/// The Projets, over the workspace context.
pub struct WorkspaceProjects(pub Arc<Workspace>);

impl Projects for WorkspaceProjects {
    fn owner_of<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>> {
        Box::pin(async move { self.0.owner_of(path).await.map_err(|e| e.to_string()) })
    }

    fn provisioning_rules<'a>(
        &'a self,
        repository: &'a str,
        run: ProvisioningRules,
    ) -> BoxFuture<'a, Result<Vec<ScopedRules>, String>> {
        Box::pin(async move {
            self.0
                .scoped_provisioning_rules(repository, run, ProvisioningRules::default())
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// The bank and the delivery, over the skill context.
pub struct ContextSkills(pub Arc<Skills>);

impl NodeSkills for ContextSkills {
    fn bank_names(&self) -> BoxFuture<'_, BTreeMap<String, String>> {
        Box::pin(async move { self.0.bank_names().await.unwrap_or_default() })
    }

    fn deliver(
        &self,
        worktree: &Path,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>> {
        self.0.deliver(worktree, run_repo_root, run_id, skills)
    }

    fn withdraw(&self, repo_root: &Path, run_id: &str) -> anyhow::Result<()> {
        self.0.withdraw(repo_root, run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::Projects;
    use pdo_workspace::adapters::{fs, git, sqlite};
    use pdo_workspace::application::ports::{ProjectEvents, ProjectRepository};
    use pdo_workspace::application::WorkspacePorts;
    use pdo_workspace::domain::project::StoredSetting;

    struct Silent;
    impl ProjectEvents for Silent {
        fn project_changed<'a>(&'a self, _project_id: &'a str) -> BoxFuture<'a, ()> {
            Box::pin(async {})
        }
    }

    /// Stage 3: the Projets as the workspace context stores them in SQLite,
    /// read back through the run context's port.
    #[tokio::test]
    async fn the_projets_are_read_from_the_workspaces_own_store() {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite::init(&db).await.unwrap();
        let store = Arc::new(sqlite::SqliteProjects::new(db.clone()));
        let workspace = Arc::new(Workspace::new(
            WorkspacePorts {
                projects: store.clone(),
                provisioning: Arc::new(sqlite::SqliteProvisioningRules::new(db.clone())),
                recent: Arc::new(sqlite::SqliteRecentRepositories::new(db.clone())),
                events: Arc::new(Silent),
                git: Arc::new(git::GitCli),
                source_tree: Arc::new(fs::FsSourceTree),
                files: Arc::new(fs::FsBrowser::new("/")),
            },
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        ));
        let project = store.create("Alpha").await.unwrap();
        store.add_member(&project.id, "/repos/front").await.unwrap();
        store.set_harness(&project.id, Some("codex")).await.unwrap();
        store
            .set_agent_choice(
                &project.id,
                Some(StoredSetting("{\"kind\":\"inherit\"}".into())),
            )
            .await
            .unwrap();

        let projects = WorkspaceProjects(workspace);
        let owner = projects.owner_of("/repos/front").await.unwrap().unwrap();
        assert_eq!(owner.id, project.id);
        assert_eq!(owner.harness.as_deref(), Some("codex"));
        assert!(projects.owner_of("/repos/other").await.unwrap().is_none());
    }
}
