//! Attach a repository path to a Projet. A path may be a member of several
//! Projets (ADR-0087); attaching it twice to the same one is a no-op.
//!
//! Bringing a repository into a Projet is judged where the repository is: a
//! member of other Projets asks its read in one of them, a path in no Projet
//! the disk's read ([`browse_disk`](super::read_access::browse_disk)).

use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::project::Project;

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::read_access::read_repository_in;
use super::workspace::Workspace;

impl Workspace {
    /// The Projet once `path` is attached; `None` when it vanished right
    /// after.
    pub async fn add_project_member(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> WorkspaceResult<Option<Project>> {
        let failed = |what: &'static str| {
            move |e: StorageError| StorageError(format!("failed to {what} project {id}: {e}"))
        };
        let path = path.trim();
        if path.is_empty() {
            return Err(WorkspaceError::Invalid("member path is required".into()));
        }
        let Some(project) = self.projects.get(id).await.map_err(failed("load"))? else {
            return Err(WorkspaceError::NotFound);
        };
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(id),
        )?;
        if !project.has_member(path) {
            self.authorize_joining(actor, path).await?;
            self.projects
                .add_member(id, path)
                .await
                .map_err(failed("add member to"))?;
        }
        self.events.project_changed(id).await;
        Ok(self.projects.get(id).await.ok().flatten())
    }

    /// Whether `actor` may bring the repository `path` into a Projet: when it
    /// is a member of Projets, if it may read it in one of them; otherwise,
    /// if it may read the disk.
    pub(crate) async fn authorize_joining(&self, actor: &Actor, path: &str) -> Result<(), Denied> {
        let projects = self.projects_of_repository(path).await;
        match self.read_in_one_of(actor, path, &projects) {
            Some(verdict) => verdict,
            None => self.authorize_browse(actor),
        }
    }

    /// Whether `actor` may take the folder `path` for the Projet `project`
    /// (a clone into it, reused or not, a new repository there): when it is
    /// a member of other Projets, if it may read it in one of them.
    pub(crate) async fn authorize_taking(
        &self,
        actor: &Actor,
        path: &str,
        project: &str,
    ) -> Result<(), Denied> {
        let mut projects = self.projects_of_repository(path).await;
        projects.retain(|id| id != project);
        self.read_in_one_of(actor, path, &projects)
            .unwrap_or(Ok(()))
    }

    /// Whether `actor` may read the repository `path` in one of `projects`;
    /// `None` when there are none.
    fn read_in_one_of(
        &self,
        actor: &Actor,
        path: &str,
        projects: &[String],
    ) -> Option<Result<(), Denied>> {
        let mut refusal = None;
        for id in projects {
            match authorize_read(self.host(), actor, &read_repository_in(path, id)) {
                Ok(()) => return Some(Ok(())),
                Err(denied) => {
                    refusal.get_or_insert(denied);
                }
            }
        }
        refusal.map(Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::read_access::browse_disk;
    use crate::application::testing::{denying, fixture, outside_of, person, refusing};

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
        let Some(project) = added else {
            panic!("expected an attach, got {added:?}")
        };
        assert_eq!(project.members, vec!["/repos/front".to_string()]);
        let again = f
            .workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        assert!(matches!(again, Some(p) if p.members.len() == 1));
        assert_eq!(f.events.changed().len(), 2);
    }

    #[tokio::test]
    async fn a_path_may_be_a_member_of_several_projets() {
        let f = fixture();
        f.projects.seed(Project::named("a", "Alpha"));
        f.projects.seed(Project::named("b", "Bravo"));
        let actor = Actor::LocalOperator;
        f.workspace
            .add_project_member(&actor, "a", "/repos/front")
            .await
            .unwrap();
        let bravo = f
            .workspace
            .add_project_member(&actor, "b", "/repos/front")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bravo.members, vec!["/repos/front".to_string()]);
        let alpha = f.projects.all().into_iter().find(|p| p.id == "a").unwrap();
        assert_eq!(alpha.members, vec!["/repos/front".to_string()]);
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

    /// Mia writes in `p` and reads the repositories of `r`; `s` holds
    /// `/repos/s`, `r` holds `/repos/shared` too.
    fn mia_in_p_reading_r() -> crate::application::testing::Fixture {
        let f = refusing(|_, operation| {
            let reads_in_r = operation.resource == ResourceKind::Repository
                && operation.verb == Verb::Read
                && operation.projects == ["r"];
            outside_of("p", operation) && !reads_in_r
        });
        f.projects.seed(Project::named("p", "Payments"));
        let mut r = Project::named("r", "Risk");
        r.members = vec!["/repos/r".into(), "/repos/shared".into()];
        f.projects.seed(r);
        let mut s = Project::named("s", "Secret");
        s.members = vec!["/repos/s".into(), "/repos/shared".into()];
        f.projects.seed(s);
        f
    }

    fn members_of(f: &crate::application::testing::Fixture, id: &str) -> Vec<String> {
        f.projects
            .all()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .members
    }

    #[tokio::test]
    async fn a_repository_of_another_projet_joins_when_it_may_be_read_there() {
        let f = mia_in_p_reading_r();
        let mia = person("mia");
        f.workspace
            .add_project_member(&mia, "p", "/repos/r")
            .await
            .unwrap();
        // Read in one of its Projets is enough.
        f.workspace
            .add_project_member(&mia, "p", "/repos/shared")
            .await
            .unwrap();
        let err = f
            .workspace
            .add_project_member(&mia, "p", "/repos/s")
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::Denied(Denied::new("refused")));
        assert_eq!(members_of(&f, "p"), ["/repos/r", "/repos/shared"]);
        let asked = f.host.asked();
        assert!(
            asked.contains(&read_repository_in("/repos/s", "s")),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn a_path_in_no_projet_joins_only_who_reads_the_disk() {
        let f = mia_in_p_reading_r();
        let err = f
            .workspace
            .add_project_member(&person("mia"), "p", "/anywhere")
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(members_of(&f, "p").is_empty());
        assert!(f.host.asked().contains(&browse_disk()));

        let f = fixture();
        f.projects.seed(Project::named("p", "Payments"));
        f.workspace
            .add_project_member(&Actor::LocalOperator, "p", "/anywhere")
            .await
            .unwrap();
        assert_eq!(members_of(&f, "p"), ["/anywhere"]);
    }
}
