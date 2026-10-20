//! Delete a Projet, free its member paths, and have what pointed at it
//! forget it.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

impl Workspace {
    pub async fn delete_project(&self, actor: &Actor, id: &str) -> WorkspaceResult<()> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Delete).on(id),
        )?;
        let deleted = self
            .projects
            .delete(id)
            .await
            .map_err(|e| StorageError(format!("failed to delete project {id}: {e}")))?;
        if !deleted {
            return Err(WorkspaceError::NotFound);
        }
        self.deletions.project_deleted(id).await;
        self.events.project_changed(id).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ports::ProjectRepository;
    use crate::application::testing::{denying, fixture};
    use crate::application::WorkspaceError;
    use crate::domain::project::Project;
    use pdo_kernel::{Actor, Verb};

    #[tokio::test]
    async fn deletes_and_announces() {
        let f = fixture();
        f.projects.seed(Project::named("p1", "Front"));
        f.workspace
            .delete_project(&Actor::LocalOperator, "p1")
            .await
            .unwrap();
        assert!(f.projects.all().is_empty());
        // CORE-96: its Runs keep it, so its name stays.
        assert_eq!(f.projects.deleted().await.unwrap()[0].name, "Front");
        assert_eq!(f.deletions.deleted(), vec!["p1".to_string()]);
        assert_eq!(f.events.changed(), vec!["p1".to_string()]);
    }

    #[tokio::test]
    async fn an_unknown_projet_is_not_found_and_announces_nothing() {
        let f = fixture();
        let err = f
            .workspace
            .delete_project(&Actor::LocalOperator, "nope")
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::NotFound);
        assert!(f.deletions.deleted().is_empty());
        assert!(f.events.changed().is_empty());
    }

    #[tokio::test]
    async fn a_refusal_keeps_the_projet() {
        let f = denying("no");
        f.projects.seed(Project::named("p1", "Front"));
        let err = f
            .workspace
            .delete_project(&Actor::LocalOperator, "p1")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert_eq!(f.projects.all().len(), 1);
        assert!(f.deletions.deleted().is_empty());
        assert_eq!(f.host.asked()[0].verb, Verb::Delete);
    }
}
