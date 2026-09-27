//! Name a new Projet: the only path that creates one (ADR-0046). The initial
//! members come with the name, so naming a group is one `Create` Opération —
//! an extension that refuses only `Update` never leaves a half-made Projet.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::{project_name, Project};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

/// What naming a Projet came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectCreated {
    Created(Project),
    /// An initial member already belongs to another Projet; nothing was written.
    Refused {
        owner_id: String,
        owner_name: String,
    },
}

impl Workspace {
    pub async fn create_project(
        &self,
        actor: &Actor,
        name: &str,
        members: &[String],
    ) -> WorkspaceResult<ProjectCreated> {
        let Some(name) = project_name(name) else {
            return Err(WorkspaceError::Invalid("project name is required".into()));
        };
        let mut paths: Vec<&str> = Vec::new();
        for path in members.iter().map(|p| p.trim()) {
            if path.is_empty() {
                return Err(WorkspaceError::Invalid("member path is required".into()));
            }
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        self.authorize(actor, Operation::new(ResourceKind::Project, Verb::Create))?;
        // The Projet does not exist yet: any owner is another Projet.
        for path in &paths {
            if let Some(owner) = self
                .projects
                .owner_of(path)
                .await
                .map_err(|e| StorageError(format!("failed to create project: {e}")))?
            {
                return Ok(ProjectCreated::Refused {
                    owner_id: owner.id,
                    owner_name: owner.name,
                });
            }
        }
        let mut project = self
            .projects
            .create(name)
            .await
            .map_err(|e| StorageError(format!("failed to create project: {e}")))?;
        for path in &paths {
            self.projects
                .add_member(&project.id, path)
                .await
                .map_err(|e| {
                    StorageError(format!(
                        "failed to add member to project {}: {e}",
                        project.id
                    ))
                })?;
        }
        if !paths.is_empty() {
            if let Ok(Some(fresh)) = self.projects.get(&project.id).await {
                project = fresh;
            }
        }
        self.events.project_changed(&project.id);
        Ok(ProjectCreated::Created(project))
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectCreated;
    use crate::application::testing::{denying, fixture};
    use crate::application::WorkspaceError;
    use crate::domain::project::Project;
    use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

    #[tokio::test]
    async fn creates_a_trimmed_projet_and_announces_it() {
        let f = fixture();
        let project = f
            .workspace
            .create_project(&Actor::LocalOperator, "  Front  ", &[])
            .await
            .unwrap();
        let ProjectCreated::Created(project) = project else {
            panic!("expected a creation, got {project:?}")
        };
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
            .create_project(&Actor::LocalOperator, "   ", &[])
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
            .create_project(&Actor::LocalOperator, "Front", &["/x".to_string()])
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

    #[tokio::test]
    async fn the_initial_members_are_part_of_the_one_create_operation() {
        let f = fixture();
        let project = f
            .workspace
            .create_project(
                &Actor::LocalOperator,
                "Front",
                &[
                    " /repos/front ".into(),
                    "/repos/back".into(),
                    "/repos/front".into(),
                ],
            )
            .await
            .unwrap();
        let ProjectCreated::Created(project) = project else {
            panic!("expected a creation, got {project:?}")
        };
        assert_eq!(
            project.members,
            vec!["/repos/front".to_string(), "/repos/back".to_string()]
        );
        let asked = f.host.asked();
        assert_eq!(
            asked.len(),
            1,
            "one Opération, not one per member: {asked:?}"
        );
        assert_eq!(
            asked[0],
            Operation::new(ResourceKind::Project, Verb::Create)
        );
        assert_eq!(f.events.changed(), vec![project.id.clone()]);
    }

    #[tokio::test]
    async fn a_member_owned_elsewhere_is_refused_by_name_before_any_write() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.workspace
            .add_project_member(&Actor::LocalOperator, "a", "/repos/front")
            .await
            .unwrap();
        let refused = f
            .workspace
            .create_project(&Actor::LocalOperator, "Bravo", &["/repos/front".into()])
            .await
            .unwrap();
        assert_eq!(
            refused,
            ProjectCreated::Refused {
                owner_id: "a".into(),
                owner_name: "Alpha".into()
            }
        );
        assert_eq!(f.projects.all().len(), 1);
    }

    #[tokio::test]
    async fn a_blank_member_path_is_refused_before_anything_is_written() {
        let f = fixture();
        let err = f
            .workspace
            .create_project(&Actor::LocalOperator, "Front", &["  ".into()])
            .await
            .unwrap_err();
        assert_eq!(
            err,
            WorkspaceError::Invalid("member path is required".into())
        );
        assert!(f.projects.all().is_empty());
    }
}
