//! Fetch every remote of a repository, then answer its refreshed branch list
//! (#802). The ONE place the daemon talks to a remote, and only on a human
//! intention (form opened, repo changed, sync clicked).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::branches::BranchList;

use super::error::{WorkspaceError, WorkspaceResult};
use super::workspace::Workspace;

impl Workspace {
    /// A failed fetch is **not** an error: the list is still the truth on disk,
    /// and it comes back with `fetch_error` set (ADR-0070 §1). The only refusals
    /// are a path that is not a repository and a list git cannot read.
    pub async fn fetch_remotes(&self, actor: &Actor, path: &str) -> WorkspaceResult<BranchList> {
        let repo = self
            .git
            .validate_repository(path)
            .map_err(WorkspaceError::Invalid)?;
        self.authorize(
            actor,
            Operation::new(ResourceKind::Repository, Verb::Action("fetch".into()))
                .on(repo.display().to_string()),
        )?;
        let fetch_error = self.git.fetch(&repo).await;
        let mut list = self.git.branches(&repo).map_err(WorkspaceError::Git)?;
        list.fetch_error = fetch_error;
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::testing::{denying, fixture};
    use crate::application::WorkspaceError;
    use crate::domain::branches::{FetchError, FetchErrorKind};
    use pdo_kernel::{Actor, Verb};

    #[tokio::test]
    async fn a_failed_fetch_still_answers_the_list() {
        let f = fixture();
        f.git.fail_fetch(FetchError {
            kind: FetchErrorKind::Network,
            message: "offline".into(),
        });
        let list = f
            .workspace
            .fetch_remotes(&Actor::LocalOperator, "/repo")
            .await
            .unwrap();
        assert_eq!(list.fetch_error.unwrap().kind, FetchErrorKind::Network);
        assert_eq!(list.branches.len(), 1);
        assert_eq!(f.git.fetched(), vec!["/repo".to_string()]);
    }

    #[tokio::test]
    async fn a_path_that_is_not_a_repository_is_invalid_and_never_fetched() {
        let f = fixture();
        let err = f
            .workspace
            .fetch_remotes(&Actor::LocalOperator, "relative")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)));
        assert!(f.git.fetched().is_empty());
    }

    #[tokio::test]
    async fn a_refusal_fetches_nothing() {
        let f = denying("offline policy");
        let err = f
            .workspace
            .fetch_remotes(&Actor::LocalOperator, "/repo")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(f.git.fetched().is_empty());
        assert_eq!(f.host.asked()[0].verb, Verb::Action("fetch".into()));
    }
}
