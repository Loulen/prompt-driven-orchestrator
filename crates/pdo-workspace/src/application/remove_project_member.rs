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
    /// Scoped to the named Projet: a path owned by a *different* Projet is left
    /// alone (an idempotent no-op), so a stale client can't strip another
    /// Projet's member.
    pub async fn remove_project_member(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> WorkspaceResult<MemberRemoved> {
        let path = path.trim();
        let owner = self
            .projects
            .owner_of(path)
            .await
            .map_err(|e| StorageError(format!("failed to resolve owner of {path}: {e}")))?;
        if owner.is_none_or(|owner| owner.id != id) {
            return match self
                .projects
                .get(id)
                .await
                .map_err(|e| StorageError(format!("failed to load project {id}: {e}")))?
            {
                Some(project) => Ok(MemberRemoved::Untouched(project)),
                None => Err(WorkspaceError::NotFound),
            };
        }
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;
        self.projects
            .remove_member(path)
            .await
            .map_err(|e| StorageError(format!("failed to remove member from project {id}: {e}")))?;
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
