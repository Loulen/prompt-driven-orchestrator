//! Fetch every remote of a repository, then answer its refreshed branch list
//! (#802). The ONE place the daemon talks to a remote, and only on a human
//! intention (form opened, repo changed, sync clicked).

use std::path::Path;

use pdo_kernel::extension_host::{ExtensionHost, GitCredentials, GitOperation, GitOperationKind};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::branches::{BranchList, FetchError};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::Git;
use super::workspace::Workspace;

/// Fetch every remote of `repo` (`kind`: a gesture's fetch, a Trigger's) for
/// `actor` in `project`: with the credentials the extensions provide for its
/// remote's URL, else the daemon's. `None` on success; a refusal names whose
/// credentials were refused.
pub async fn fetch_for(
    git: &dyn Git,
    host: &dyn ExtensionHost,
    repo: &Path,
    actor: &Actor,
    kind: GitOperationKind,
    project: Option<&str>,
) -> Option<FetchError> {
    let url = git.remote_url(repo);
    let credentials = match &url {
        Some(url) => {
            let mut operation = GitOperation::new(actor.clone(), url, kind);
            if let Some(project) = project {
                operation = operation.in_project(project);
            }
            host.git_credentials(&operation)
        }
        None => GitCredentials::daemon(),
    };
    let error = git.fetch(repo, &credentials).await?;
    Some(match &url {
        Some(url) => error.naming_credentials(url, &credentials),
        None => error,
    })
}

impl Workspace {
    /// A failed fetch is **not** an error: the list is still the truth on disk,
    /// and it comes back with `fetch_error` set (ADR-0070 §1). The only refusals
    /// are a path that is not a repository and a list git cannot read.
    ///
    /// The fetch runs with the credentials the extensions provide for `actor`
    /// (in the repository's Projet when it is in one only), else the daemon's;
    /// a refusal names which.
    pub async fn fetch_remotes(&self, actor: &Actor, path: &str) -> WorkspaceResult<BranchList> {
        let repo = self
            .git
            .validate_repository(path)
            .map_err(WorkspaceError::Invalid)?;
        let path = repo.display().to_string();
        let projects = self.projects_of_repository(&path).await;
        let project = match projects.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        };
        self.authorize(
            actor,
            Operation::new(ResourceKind::Repository, Verb::Action("fetch".into()))
                .on(path)
                .in_projects(projects),
        )?;
        let fetch_error = fetch_for(
            &*self.git,
            self.host(),
            &repo,
            actor,
            GitOperationKind::Fetch,
            project.as_deref(),
        )
        .await;
        let mut list = self.git.branches(&repo).map_err(WorkspaceError::Git)?;
        list.fetch_error = fetch_error;
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::testing::{alice, denying, fixture, ALICE_GIT_ENV};
    use crate::application::WorkspaceError;
    use crate::domain::branches::{FetchError, FetchErrorKind};
    use pdo_kernel::extension_host::{GitCredentials, GitOperationKind};
    use pdo_kernel::{Actor, Verb};

    #[tokio::test]
    async fn the_fetch_runs_with_the_credentials_provided_for_the_actor() {
        let f = fixture();
        f.projects
            .seed(crate::domain::project::Project::named("a", "Alpha"));
        f.workspace
            .add_project_member(&Actor::LocalOperator, "a", "/repo")
            .await
            .unwrap();
        f.workspace.fetch_remotes(&alice(), "/repo").await.unwrap();
        let asked = f.host.git_asked();
        assert_eq!(asked.len(), 1);
        assert_eq!(
            (&asked[0].actor, asked[0].kind, asked[0].project.as_deref()),
            (&alice(), GitOperationKind::Fetch, Some("a"))
        );
        assert_eq!(asked[0].url, "https://me:token@example.com/acme/repo.git");
        let used = f.git.credentials();
        assert_eq!(
            used[0].env.get(ALICE_GIT_ENV.0).map(String::as_str),
            Some(ALICE_GIT_ENV.1)
        );
    }

    #[tokio::test]
    async fn a_refused_fetch_names_whose_credentials() {
        let f = fixture();
        f.git.fail_fetch(FetchError {
            kind: FetchErrorKind::Auth,
            message: "fatal: Authentication failed".into(),
        });
        let mine = f.workspace.fetch_remotes(&alice(), "/repo").await.unwrap();
        assert_eq!(
            mine.fetch_error.unwrap().message,
            "The credentials provided for Alice were refused for example.com: fatal: Authentication failed"
        );
        let daemon = f
            .workspace
            .fetch_remotes(&Actor::LocalOperator, "/repo")
            .await
            .unwrap();
        assert!(daemon
            .fetch_error
            .unwrap()
            .message
            .starts_with("The daemon's git credentials were refused for example.com: "));
        assert_eq!(f.git.credentials()[1], GitCredentials::daemon());
    }

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

    #[tokio::test]
    async fn a_fetch_is_in_the_projet_owning_the_repository() {
        let f = fixture();
        f.projects
            .seed(crate::domain::project::Project::named("a", "Alpha"));
        f.workspace
            .add_project_member(&Actor::LocalOperator, "a", "/repo")
            .await
            .unwrap();
        f.workspace
            .fetch_remotes(&Actor::LocalOperator, "/repo")
            .await
            .unwrap();
        f.workspace
            .fetch_remotes(&Actor::LocalOperator, "/elsewhere")
            .await
            .unwrap();
        let asked = f.host.asked();
        assert_eq!(asked[0].projects, vec!["a".to_string()], "the member's add");
        assert_eq!(asked[1].projects, vec!["a".to_string()]);
        assert!(asked[2].is_outside_projects());
    }
}
