//! The reads of the workspace that go through the Autorisation d'opération
//! (CORE-5): listing and reading the Projets, browsing the disk (the loupe of
//! the New Run form, the explorer of the settings), the recent repositories
//! (the prefill of the New Run form).
//!
//! A Projet is read in itself; the disk is read in no Projet. A recent
//! repository comes from a Run: it is listed when its last Run may be read,
//! in the Projet of the repository, as the Run list does. Without extension,
//! nothing is asked.

use pdo_kernel::attributes::AttributeFilter;
use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StoreResult;
use super::queries::RECENT_REPOSITORIES;
use super::workspace::Workspace;
use crate::domain::project::Project;

/// Reading the Projet `id`, in that Projet.
pub fn read_project(id: &str) -> Operation {
    Operation::new(ResourceKind::Project, Verb::Read)
        .on(id)
        .in_projects([id])
}

/// Reading the Run `run_id`, in `projects`.
pub fn read_run_in(run_id: &str, projects: Vec<String>) -> Operation {
    Operation::new(ResourceKind::Run, Verb::Read)
        .on(run_id)
        .in_projects(projects)
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

    /// The last repositories Runs worked in, most recent first, each once:
    /// only those whose last Run `actor` may read, at most
    /// [`RECENT_REPOSITORIES`].
    pub async fn recent_repositories(&self, actor: &Actor) -> StoreResult<Vec<String>> {
        let mut kept = Vec::new();
        for recent in self.recent.recent().await? {
            if kept.len() == RECENT_REPOSITORIES {
                break;
            }
            if !self.host().allows_everything() {
                let projects = self.projects_of_repository(&recent.path).await;
                let operation = read_run_in(&recent.run_id, projects);
                if authorize_read(self.host(), actor, &operation).is_err() {
                    continue;
                }
            }
            kept.push(recent.path);
        }
        Ok(kept)
    }

    /// Whether `actor` may browse the disk.
    pub fn authorize_browse(&self, actor: &Actor) -> Result<(), Denied> {
        authorize_read(self.host(), actor, &browse_disk())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::RecentRepository;
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

    fn with_members(mut project: Project, members: &[&str]) -> Project {
        project.members = members.iter().map(|m| m.to_string()).collect();
        project
    }

    #[tokio::test]
    async fn the_recent_list_is_capped() {
        let f = fixture();
        f.recent.set(
            (0..8)
                .map(|i| RecentRepository::new(format!("/r{i}"), format!("run-{i}")))
                .collect(),
        );
        let actor = Actor::LocalOperator;
        let recent = f.workspace.recent_repositories(&actor).await.unwrap();
        assert_eq!(recent, ["/r0", "/r1", "/r2", "/r3", "/r4"]);
    }

    #[tokio::test]
    async fn the_recent_list_skips_the_repositories_of_runs_the_actor_may_not_read() {
        let f = denying_reads_in(&["p2"]);
        f.projects
            .seed(with_members(Project::named("p1", "Front"), &["/r1"]));
        f.projects
            .seed(with_members(Project::named("p2", "Back"), &["/r2"]));
        f.recent.set(vec![
            RecentRepository::new("/r2", "run-b"),
            RecentRepository::new("/r1", "run-a"),
            RecentRepository::new("/loose", "run-c"),
        ]);
        let actor = Actor::LocalOperator;
        let recent = f.workspace.recent_repositories(&actor).await.unwrap();
        assert_eq!(recent, ["/r1", "/loose"]);
        let asked = f.host.asked();
        assert!(
            asked.contains(&read_run_in("run-b", vec!["p2".into()])),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn a_repository_spelled_otherwise_stays_in_its_projet() {
        let f = denying_reads_in(&["p2"]);
        f.projects
            .seed(with_members(Project::named("p2", "Back"), &["/r2"]));
        f.recent.set(vec![
            RecentRepository::new("/r2/", "run-a"),
            RecentRepository::new("/r2/.", "run-b"),
        ]);
        let actor = Actor::LocalOperator;
        let recent = f.workspace.recent_repositories(&actor).await.unwrap();
        assert!(recent.is_empty(), "{recent:?}");
        let asked = f.host.asked();
        assert!(
            asked.contains(&read_run_in("run-a", vec!["p2".into()])),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn the_cap_counts_only_what_the_actor_may_read() {
        let f = denying_reads_in(&["p2"]);
        let hidden = ["/hidden0", "/hidden1", "/hidden2"];
        f.projects
            .seed(with_members(Project::named("p2", "Back"), &hidden));
        let mut recent: Vec<_> = hidden
            .iter()
            .enumerate()
            .map(|(i, path)| RecentRepository::new(*path, format!("run-h{i}")))
            .collect();
        recent.extend((0..6).map(|i| RecentRepository::new(format!("/r{i}"), format!("run-{i}"))));
        f.recent.set(recent);
        let actor = Actor::LocalOperator;
        let kept = f.workspace.recent_repositories(&actor).await.unwrap();
        assert_eq!(kept, ["/r0", "/r1", "/r2", "/r3", "/r4"]);
    }
}
