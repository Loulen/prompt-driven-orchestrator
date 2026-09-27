//! Create a git repository from a parent, a name and initial files (#824).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::scaffold::{ScaffoldFile, ScaffoldOutcome};

use super::error::{WorkspaceError, WorkspaceResult};
use super::workspace::Workspace;

impl Workspace {
    /// Deliberately **not** recorded in the recent repositories: that list is
    /// built from the Runs, and a scaffolded repository has never run anything.
    pub async fn create_repository(
        &self,
        actor: &Actor,
        parent: &str,
        name: &str,
        files: &[ScaffoldFile],
    ) -> WorkspaceResult<ScaffoldOutcome> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Repository, Verb::Create),
        )?;
        self.git
            .create_repository(parent, name, files)
            .map_err(WorkspaceError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::testing::{denying, fixture};
    use crate::application::WorkspaceError;
    use pdo_kernel::{Actor, ResourceKind, Verb};

    #[tokio::test]
    async fn creates_under_the_parent() {
        let f = fixture();
        let outcome = f
            .workspace
            .create_repository(&Actor::LocalOperator, "/tmp", "tuto", &[])
            .await
            .unwrap();
        assert_eq!(outcome.path, std::path::PathBuf::from("/tmp/tuto"));
        assert!(outcome.created);
    }

    #[tokio::test]
    async fn a_git_refusal_is_invalid_and_a_denial_creates_nothing() {
        let f = fixture();
        let err = f
            .workspace
            .create_repository(&Actor::LocalOperator, "relative", "tuto", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)));

        let f = denying("no");
        let err = f
            .workspace
            .create_repository(&Actor::LocalOperator, "/tmp", "tuto", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(f.git.created().is_empty());
        let asked = &f.host.asked()[0];
        assert_eq!(
            (asked.resource, &asked.verb),
            (ResourceKind::Repository, &Verb::Create)
        );
    }
}
