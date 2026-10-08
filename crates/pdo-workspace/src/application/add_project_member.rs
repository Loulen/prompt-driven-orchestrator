//! Attach a repository path to a Projet: at most one Projet per path, and the
//! refusal names the owner (ADR-0046).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::{membership_verdict, AddMember, Project};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

/// What attaching a path came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberAdded {
    /// Attached (or already a member of this Projet). `None` when the Projet
    /// vanished right after.
    Attached(Option<Project>),
    /// The path belongs to another Projet; nothing was written.
    Refused {
        owner_id: String,
        owner_name: String,
    },
}

impl Workspace {
    pub async fn add_project_member(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> WorkspaceResult<MemberAdded> {
        let failed = |what: &'static str| {
            move |e: StorageError| StorageError(format!("failed to {what} project {id}: {e}"))
        };
        let path = path.trim();
        if path.is_empty() {
            return Err(WorkspaceError::Invalid("member path is required".into()));
        }
        if self
            .projects
            .get(id)
            .await
            .map_err(failed("load"))?
            .is_none()
        {
            return Err(WorkspaceError::NotFound);
        }
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;
        let owner = self
            .projects
            .owner_of(path)
            .await
            .map_err(failed("add member to"))?;
        match membership_verdict(owner.as_ref(), id) {
            Some(AddMember::Refused {
                owner_id,
                owner_name,
            }) => {
                return Ok(MemberAdded::Refused {
                    owner_id,
                    owner_name,
                })
            }
            Some(_) => {}
            None => self
                .projects
                .add_member(id, path)
                .await
                .map_err(failed("add member to"))?,
        }
        self.events.project_changed(id).await;
        Ok(MemberAdded::Attached(
            self.projects.get(id).await.ok().flatten(),
        ))
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
        let MemberAdded::Attached(Some(project)) = added else {
            panic!("expected an attach, got {added:?}")
        };
        assert_eq!(project.members, vec!["/repos/front".to_string()]);
        let again = f
            .workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        assert!(matches!(again, MemberAdded::Attached(Some(p)) if p.members.len() == 1));
        assert_eq!(f.events.changed().len(), 2);
    }

    #[tokio::test]
    async fn a_path_owned_elsewhere_is_refused_by_name_before_any_write() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.projects.seed(Project::named("b", "Bravo"));
        let actor = Actor::LocalOperator;
        f.workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        let refused = f
            .workspace
            .add_project_member(&actor, "b", "/repos/front")
            .await
            .unwrap();
        assert_eq!(
            refused,
            MemberAdded::Refused {
                owner_id: "a".into(),
                owner_name: "Alpha".into()
            }
        );
        let bravo = f.projects.all().into_iter().find(|p| p.id == "b").unwrap();
        assert!(bravo.members.is_empty());
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
