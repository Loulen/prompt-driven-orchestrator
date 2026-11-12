//! Name a new Projet: the only path that creates one (ADR-0046). The initial
//! members come with the name, so naming a group is one `Create` Opération —
//! an extension that refuses only `Update` never leaves a half-made Projet.

use pdo_kernel::attributes::{stamp, EntityRef};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::{project_name, Project};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::workspace::Workspace;

impl Workspace {
    pub async fn create_project(
        &self,
        actor: &Actor,
        name: &str,
        members: &[String],
    ) -> WorkspaceResult<Project> {
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
        // A member of another Projet joins this one too (ADR-0087), when it
        // may be read there.
        self.authorize(actor, Operation::new(ResourceKind::Project, Verb::Create))?;
        for path in &paths {
            self.authorize_joining(actor, path).await?;
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
        let entity = EntityRef::new(ResourceKind::Project, project.id.clone());
        project.attributes = stamp(self.host(), actor, entity).await;
        self.events.project_changed(&project.id).await;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            project.members,
            vec!["/repos/front".to_string(), "/repos/back".to_string()]
        );
        // Each member is only read where it is (the disk, here).
        let asked: Vec<_> = f
            .host
            .asked()
            .into_iter()
            .filter(|o| !o.is_read())
            .collect();
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
    async fn a_member_of_another_projet_joins_this_one_too() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.workspace
            .add_project_member(&Actor::LocalOperator, "a", "/repos/front")
            .await
            .unwrap();
        let bravo = f
            .workspace
            .create_project(&Actor::LocalOperator, "Bravo", &["/repos/front".into()])
            .await
            .unwrap();
        assert_eq!(bravo.members, vec!["/repos/front".to_string()]);
        let alpha = f.projects.all().into_iter().find(|p| p.id == "a").unwrap();
        assert_eq!(alpha.members, vec!["/repos/front".to_string()]);
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

    #[tokio::test]
    async fn an_initial_member_is_judged_where_it_is_before_anything_is_written() {
        use crate::application::testing::{outside_of, person, refusing};
        let f = refusing(|_, operation| {
            operation.resource == ResourceKind::Repository && outside_of("p", operation)
        });
        let mut s = Project::named("s", "Secret");
        s.members = vec!["/repos/s".into()];
        f.projects.seed(s);
        let mia = person("mia");
        for member in ["/repos/s", "/anywhere"] {
            let err = f
                .workspace
                .create_project(&mia, "Mine", &[member.into()])
                .await
                .unwrap_err();
            assert!(
                matches!(err, WorkspaceError::Denied(_)),
                "{member}: {err:?}"
            );
        }
        assert_eq!(f.projects.all().len(), 1, "nothing created");
        let mine = f.workspace.create_project(&mia, "Mine", &[]).await.unwrap();
        assert!(mine.members.is_empty());
    }
}
