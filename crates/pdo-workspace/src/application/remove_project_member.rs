//! Detach a repository path from a Projet.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::Project;

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

/// What detaching a path came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberRemoved {
    /// Detached; the Projet as it is now (`None` if it vanished right after).
    Detached(Option<Project>),
    /// The path was not this Projet's: nothing changed. The Projet as it is.
    Untouched(Project),
}

impl Workspace {
    /// Scoped to the named Projet: the path stays a member of every other
    /// Projet, and a path that is not this Projet's is an idempotent no-op.
    /// Detaching the Projet's default primary repository clears it.
    pub async fn remove_project_member(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> WorkspaceResult<MemberRemoved> {
        let path = path.trim();
        let failed = |e: StorageError| {
            StorageError(format!("failed to remove member from project {id}: {e}"))
        };
        let Some(project) = self.projects.get(id).await.map_err(failed)? else {
            return Err(WorkspaceError::NotFound);
        };
        if !project.has_member(path) {
            return Ok(MemberRemoved::Untouched(project));
        }
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;
        self.projects
            .remove_member(id, path)
            .await
            .map_err(failed)?;
        if project.primary_repo.as_deref() == Some(path) {
            self.projects
                .set_primary_repo(id, None)
                .await
                .map_err(failed)?;
        }
        self.events.project_changed(id).await;
        Ok(MemberRemoved::Detached(
            self.projects.get(id).await.ok().flatten(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::fixture;

    #[tokio::test]
    async fn detaches_its_own_member_and_announces() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        let actor = Actor::LocalOperator;
        f.workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        let removed = f
            .workspace
            .remove_project_member(&actor, "a", " /repos/front ")
            .await
            .unwrap();
        assert!(matches!(removed, MemberRemoved::Detached(Some(p)) if p.members.is_empty()));
        assert_eq!(f.events.changed().len(), 2);
    }

    #[tokio::test]
    async fn another_projets_member_is_left_alone() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.projects.seed(Project::named("b", "Bravo"));
        let actor = Actor::LocalOperator;
        f.workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        let removed = f
            .workspace
            .remove_project_member(&actor, "b", "/repos/front")
            .await
            .unwrap();
        assert!(matches!(removed, MemberRemoved::Untouched(p) if p.id == "b"));
        let alpha = f.projects.all().into_iter().find(|p| p.id == "a").unwrap();
        assert_eq!(alpha.members.len(), 1);
    }

    #[tokio::test]
    async fn a_shared_path_leaves_one_projet_and_stays_in_the_other() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.projects.seed(Project::named("b", "Bravo"));
        let actor = Actor::LocalOperator;
        for id in ["a", "b"] {
            f.workspace
                .add_project_member(&actor, id, "/repos/shared")
                .await
                .unwrap();
        }
        f.workspace
            .remove_project_member(&actor, "a", "/repos/shared")
            .await
            .unwrap();
        let members = |id: &str| {
            f.projects
                .all()
                .into_iter()
                .find(|p| p.id == id)
                .unwrap()
                .members
        };
        assert!(members("a").is_empty());
        assert_eq!(members("b"), vec!["/repos/shared".to_string()]);
    }

    #[tokio::test]
    async fn detaching_the_default_primary_clears_it() {
        let f = fixture();
        let mut alpha = Project::named("a", "Alpha");
        alpha.members = vec!["/repos/web".into(), "/repos/api".into()];
        alpha.primary_repo = Some("/repos/api".into());
        f.projects.seed(alpha);
        let removed = f
            .workspace
            .remove_project_member(&Actor::LocalOperator, "a", "/repos/api")
            .await
            .unwrap();
        let MemberRemoved::Detached(Some(project)) = removed else {
            panic!("expected a detach, got {removed:?}")
        };
        assert_eq!(project.primary_repo, None);
        assert_eq!(project.default_primary(), Some("/repos/web"));
    }

    #[tokio::test]
    async fn an_unknown_projet_is_not_found() {
        let f = fixture();
        let err = f
            .workspace
            .remove_project_member(&Actor::LocalOperator, "nope", "/x")
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::NotFound);
    }
}
