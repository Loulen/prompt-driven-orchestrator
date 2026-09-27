//! Name a new Projet: the only path that creates one (ADR-0046).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::{project_name, Project};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

impl Workspace {
    pub async fn create_project(&self, actor: &Actor, name: &str) -> WorkspaceResult<Project> {
        let Some(name) = project_name(name) else {
            return Err(WorkspaceError::Invalid("project name is required".into()));
        };
        self.authorize(actor, Operation::new(ResourceKind::Project, Verb::Create))?;
        let project = self
            .projects
            .create(name)
            .await
            .map_err(|e| StorageError(format!("failed to create project: {e}")))?;
        self.events.project_changed(&project.id);
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::testing::{denying, fixture};
    use crate::application::WorkspaceError;
    use pdo_kernel::Actor;

    #[tokio::test]
    async fn creates_a_trimmed_projet_and_announces_it() {
        let f = fixture();
        let project = f
            .workspace
            .create_project(&Actor::LocalOperator, "  Front  ")
            .await
            .unwrap();
        assert_eq!(project.name, "Front");
        assert!(project.members.is_empty());
        assert_eq!(f.events.changed(), vec![project.id.clone()]);
        assert_eq!(f.projects.all().len(), 1);
    }

    #[tokio::test]
    async fn a_blank_name_is_refused_before_anything_is_written() {
        let f = fixture();
        let err = f
            .workspace
            .create_project(&Actor::LocalOperator, "   ")
            .await
            .unwrap_err();
        assert_eq!(
            err,
            WorkspaceError::Invalid("project name is required".into())
        );
        assert!(f.projects.all().is_empty());
    }

    #[tokio::test]
    async fn a_refusal_writes_nothing_and_carries_the_reason() {
        let f = denying("admins only");
        let err = f
            .workspace
            .create_project(&Actor::LocalOperator, "Front")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(d) if d.reason == "admins only"));
        assert!(f.projects.all().is_empty());
        assert!(f.events.changed().is_empty());
        assert_eq!(
            f.host.asked()[0].resource,
            pdo_kernel::ResourceKind::Project
        );
    }
}
