//! The reads of the workspace that go through the Autorisation d'opération
//! (CORE-5): listing and reading the Projets, browsing the disk (the loupe of
//! the New Run form, the explorer of the settings).
//!
//! A Projet is read in itself; the disk is read in no Projet. Without
//! extension, nothing is asked.

use pdo_kernel::attributes::AttributeFilter;
use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StoreResult;
use super::workspace::Workspace;
use crate::domain::project::Project;

/// Reading the Projet `id`, in that Projet.
pub fn read_project(id: &str) -> Operation {
    Operation::new(ResourceKind::Project, Verb::Read)
        .on(id)
        .in_projects([id])
}

/// Browsing the disk for a repository: in no Projet.
pub fn browse_disk() -> Operation {
    Operation::new(ResourceKind::Repository, Verb::Read)
}

impl Workspace {
    /// Every Projet `actor` may read, carrying the attributes `filter` asks
    /// for, each with its attributes.
    pub async fn readable_projects(
        &self,
        actor: &Actor,
        filter: AttributeFilter,
    ) -> StoreResult<Vec<Project>> {
        let projects = self.projects_carrying(filter).await?;
        Ok(projects
            .into_iter()
            .filter(|project| {
                authorize_read(self.host(), actor, &read_project(&project.id)).is_ok()
            })
            .collect())
    }

    /// The Projet `id` with its attributes, when `actor` may read it.
    pub async fn readable_project(&self, actor: &Actor, id: &str) -> WorkspaceResult<Project> {
        authorize_read(self.host(), actor, &read_project(id)).map_err(WorkspaceError::Denied)?;
        self.attributed_project(id)
            .await
            .map_err(WorkspaceError::Storage)?
            .ok_or(WorkspaceError::NotFound)
    }

    /// Whether `actor` may browse the disk.
    pub fn authorize_browse(&self, actor: &Actor) -> Result<(), Denied> {
        authorize_read(self.host(), actor, &browse_disk())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{denying, denying_reads_in, fixture};

    #[tokio::test]
    async fn the_list_keeps_the_projets_the_actor_may_read() {
        let f = denying_reads_in(&["p2"]);
        f.projects.seed(Project::named("p1", "Front"));
        f.projects.seed(Project::named("p2", "Back"));
        let actor = Actor::LocalOperator;
        let names: Vec<_> = f
            .workspace
            .readable_projects(&actor, AttributeFilter::default())
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Front"]);
        let asked = f.host.asked();
        assert!(asked.contains(&read_project("p2")), "{asked:?}");
        assert_eq!(read_project("p2").projects, ["p2"]);
    }

    #[tokio::test]
    async fn one_projet_is_refused_or_missing() {
        let f = denying_reads_in(&["p2"]);
        f.projects.seed(Project::named("p1", "Front"));
        f.projects.seed(Project::named("p2", "Back"));
        let actor = Actor::LocalOperator;
        let p1 = f.workspace.readable_project(&actor, "p1").await.unwrap();
        assert_eq!(p1.name, "Front");
        assert!(matches!(
            f.workspace.readable_project(&actor, "p2").await,
            Err(WorkspaceError::Denied(_))
        ));
        assert!(matches!(
            f.workspace.readable_project(&actor, "nope").await,
            Err(WorkspaceError::NotFound)
        ));
    }

    #[test]
    fn browsing_the_disk_is_a_read_in_no_projet() {
        let actor = Actor::LocalOperator;
        assert_eq!(fixture().workspace.authorize_browse(&actor), Ok(()));
        let f = denying("administrators only");
        assert_eq!(
            f.workspace.authorize_browse(&actor),
            Err(Denied {
                reason: "administrators only".into()
            })
        );
        let asked = f.host.asked();
        assert_eq!(asked, [browse_disk()]);
        assert!(asked[0].is_outside_projects());
    }
}
