//! Fast-forward a local branch onto its tracking branch, on an explicit click
//! (#803, ADR-0070 §2).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::fast_forward::{FastForwardOk, FastForwardRefusedBody};

use super::error::{WorkspaceError, WorkspaceResult};
use super::workspace::Workspace;

/// Both outcomes carry the branch list read AFTER the attempt, so a 200 answers
/// with the branch already moved and a 409 with whatever the race left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FastForwarded {
    Done(FastForwardOk),
    Refused(FastForwardRefusedBody),
}

impl Workspace {
    pub async fn fast_forward_branch(
        &self,
        actor: &Actor,
        path: &str,
        branch: &str,
    ) -> WorkspaceResult<FastForwarded> {
        let repo = self
            .git
            .validate_repository(path)
            .map_err(WorkspaceError::Invalid)?;
        let branch = branch.trim();
        if branch.is_empty() {
            return Err(WorkspaceError::Invalid("branch is required".into()));
        }
        // A remote-tracking ref is refused up front: it is the upstream, there is
        // nothing to move.
        if !self.git.is_local_branch(&repo, branch) {
            return Err(WorkspaceError::Invalid(format!(
                "'{branch}' is not a local branch of {}",
                repo.display()
            )));
        }
        let path = repo.display().to_string();
        let projects = self.projects_of_repository(&path).await;
        self.authorize(
            actor,
            Operation::new(
                ResourceKind::Repository,
                Verb::Action("fast_forward".into()),
            )
            .on(path)
            .in_projects(projects),
        )?;
        let outcome = self.git.fast_forward(&repo, branch);
        let list = self.git.branches(&repo).map_err(WorkspaceError::Git)?;
        Ok(match outcome {
            Ok(fast_forward) => FastForwarded::Done(FastForwardOk { fast_forward, list }),
            Err(refusal) => FastForwarded::Refused(FastForwardRefusedBody { refusal, list }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{denying, fixture};
    use crate::domain::fast_forward::FastForwardRefusal;

    #[tokio::test]
    async fn a_refused_fast_forward_still_answers_the_list() {
        let f = fixture();
        let outcome = f
            .workspace
            .fast_forward_branch(&Actor::LocalOperator, "/repo", "up-to-date")
            .await
            .unwrap();
        let FastForwarded::Refused(body) = outcome else {
            panic!("expected a refusal")
        };
        assert_eq!(body.refusal.reason, FastForwardRefusal::UpToDate);
        assert_eq!(body.list.branches.len(), 1);
    }

    #[tokio::test]
    async fn a_behind_branch_moves() {
        let f = fixture();
        let outcome = f
            .workspace
            .fast_forward_branch(&Actor::LocalOperator, "/repo", " main ")
            .await
            .unwrap();
        assert!(matches!(outcome, FastForwarded::Done(ok) if ok.fast_forward.branch == "main"));
        assert_eq!(f.git.moved(), vec!["main".to_string()]);
    }

    #[tokio::test]
    async fn a_blank_or_remote_branch_is_invalid_before_anything_moves() {
        let f = fixture();
        let actor = Actor::LocalOperator;
        assert_eq!(
            f.workspace
                .fast_forward_branch(&actor, "/repo", " ")
                .await
                .unwrap_err(),
            WorkspaceError::Invalid("branch is required".into())
        );
        assert_eq!(
            f.workspace
                .fast_forward_branch(&actor, "/repo", "origin/main")
                .await
                .unwrap_err(),
            WorkspaceError::Invalid("'origin/main' is not a local branch of /repo".into())
        );
        assert!(f.git.moved().is_empty());
    }

    #[tokio::test]
    async fn a_refusal_moves_nothing() {
        let f = denying("frozen");
        let err = f
            .workspace
            .fast_forward_branch(&Actor::LocalOperator, "/repo", "main")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(f.git.moved().is_empty());
        assert_eq!(f.host.asked()[0].target.as_deref(), Some("/repo"));
    }
}
