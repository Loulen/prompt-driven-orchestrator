//! Attach a repository path to a Projet. A path may be a member of several
//! Projets (ADR-0087); attaching it twice to the same one is a no-op.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::Project;

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

impl Workspace {
    /// The Projet once `path` is attached; `None` when it vanished right
    /// after.
    pub async fn add_project_member(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> WorkspaceResult<Option<Project>> {
        let failed = |what: &'static str| {
            move |e: StorageError| StorageError(format!("failed to {what} project {id}: {e}"))
        };
        let path = path.trim();
        if path.is_empty() {
            return Err(WorkspaceError::Invalid("member path is required".into()));
        }
        let Some(project) = self.projects.get(id).await.map_err(failed("load"))? else {
            return Err(WorkspaceError::NotFound);
        };
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;
        if !project.has_member(path) {
            self.projects
                .add_member(id, path)
                .await
                .map_err(failed("add member to"))?;
        }
        self.events.project_changed(id).await;
        Ok(self.projects.get(id).await.ok().flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{denying, fixture};

    #[tokio::test]
    async fn attaches_a_trimmed_path_idempotently() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        let actor = Actor::LocalOperator;
        let added = f
            .workspace
            .add_project_member(&actor, "a", " /repos/front ")
            .await
            .unwrap();
        let Some(project) = added else {
            panic!("expected an attach, got {added:?}")
        };
        assert_eq!(project.members, vec!["/repos/front".to_string()]);
        let again = f
            .workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        assert!(matches!(again, Some(p) if p.members.len() == 1));
        assert_eq!(f.events.changed().len(), 2);
    }

    #[tokio::test]
    async fn a_path_may_be_a_member_of_several_projets() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.projects.seed(Project::named("b", "Bravo"));
        let actor = Actor::LocalOperator;
        f.workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        let bravo = f
            .workspace
            .add_project_member(&actor, "b", "/repos/front")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bravo.members, vec!["/repos/front".to_string()]);
        let alpha = f.projects.all().into_iter().find(|p| p.id == "a").unwrap();
        assert_eq!(alpha.members, vec!["/repos/front".to_string()]);
    }

    #[tokio::test]
    async fn a_blank_path_or_an_unknown_projet_is_refused() {
        let f = fixture();
        let actor = Actor::LocalOperator;
        assert_eq!(
            f.workspace
                .add_project_member(&actor, "a", "  ")
                .await
                .unwrap_err(),
            WorkspaceError::Invalid("member path is required".into())
        );
        assert_eq!(
            f.workspace
                .add_project_member(&actor, "a", "/x")
                .await
                .unwrap_err(),
            WorkspaceError::NotFound
        );
    }

    #[tokio::test]
    async fn a_refusal_attaches_nothing() {
        let f = denying("no");
        f.projects.seed(Project::named("a", "Alpha"));
        let err = f
            .workspace
            .add_project_member(&Actor::LocalOperator, "a", "/x")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(f.projects.all()[0].members.is_empty());
    }
}
