//! Create a git repository from a parent, a name and initial files (#824).
//!
//! A new empty repository for a Projet is judged in that Projet, like a
//! clone: it is made in the clones folder, and joins the Projet. Another
//! parent reaches the disk (its read); a repository already there joins only
//! who may bring it in ([`Workspace::authorize_joining`]).

use std::path::Path;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::scaffold::{ScaffoldFile, ScaffoldOutcome};

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::StorageError;
use super::read_access::create_repository_in;
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

    /// Create `<parent>/<name>` for the Projet `project_id` and add it there;
    /// `parent` absent → the clones folder.
    pub async fn create_project_repository(
        &self,
        actor: &Actor,
        project_id: &str,
        parent: Option<&str>,
        name: &str,
        files: &[ScaffoldFile],
    ) -> WorkspaceResult<ScaffoldOutcome> {
        let Some(project) = self.projects.get(project_id).await? else {
            return Err(WorkspaceError::NotFound);
        };
        self.authorize(actor, create_repository_in(project_id))?;
        self.authorize(
            actor,
            Operation::new(ResourceKind::Project, Verb::Update).on(project_id),
        )?;
        let folder = self
            .clones_folder
            .folder()
            .await
            .map_err(WorkspaceError::Invalid)?;
        let parent = match parent.map(str::trim).filter(|p| !p.is_empty()) {
            Some(parent) if Path::new(parent.trim_end_matches('/')) != folder => {
                self.authorize_browse(actor)?;
                parent.to_string()
            }
            _ => folder.to_string_lossy().into_owned(),
        };
        // A folder of another Projet is taken only by who reads it there.
        let target = Path::new(&parent).join(name.trim());
        self.authorize_taking(actor, &target.to_string_lossy(), project_id)
            .await?;
        let outcome = self
            .git
            .create_repository(&parent, name, files)
            .map_err(WorkspaceError::Invalid)?;
        let path = outcome.path.to_string_lossy().into_owned();
        if project.has_member(&path) {
            return Ok(outcome);
        }
        if !outcome.created {
            // Already there: brought in, not created.
            self.authorize_joining(actor, &path).await?;
        }
        self.projects
            .add_member(project_id, &path)
            .await
            .map_err(|e| {
                StorageError(format!("failed to add member to project {project_id}: {e}"))
            })?;
        self.events.project_changed(project_id).await;
        Ok(outcome)
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
            (asked.resource.clone(), &asked.verb),
            (ResourceKind::Repository, &Verb::Create)
        );
    }

    use crate::application::read_access::create_repository_in;
    use crate::application::testing::{outside_of, person, refusing, Fixture};
    use crate::domain::project::Project;

    /// Mia may only write in `p`; `/clones/taken` already is a repository of `s`.
    fn mia_in_p() -> Fixture {
        let f = refusing(|_, operation| outside_of("p", operation));
        f.projects.seed(Project::named("p", "Payments"));
        let mut s = Project::named("s", "Secret");
        s.members = vec!["/clones/taken".into()];
        f.projects.seed(s);
        f.git.reuse("/clones/taken");
        f
    }

    fn members_of(f: &Fixture, id: &str) -> Vec<String> {
        f.projects
            .all()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .members
    }

    #[tokio::test]
    async fn a_new_repository_for_a_projet_is_made_in_the_clones_folder_and_joins_it() {
        let f = mia_in_p();
        let mia = person("mia");
        let outcome = f
            .workspace
            .create_project_repository(&mia, "p", None, "ledger", &[])
            .await
            .unwrap();
        assert_eq!(outcome.path, std::path::PathBuf::from("/clones/ledger"));
        assert_eq!(members_of(&f, "p"), ["/clones/ledger"]);
        assert_eq!(f.events.changed(), ["p"]);
        assert!(f.host.asked().contains(&create_repository_in("p")));
        // The clones folder named as the parent is no folder of hers.
        f.workspace
            .create_project_repository(&mia, "p", Some("/clones/"), "books", &[])
            .await
            .unwrap();
        assert_eq!(members_of(&f, "p"), ["/clones/ledger", "/clones/books"]);
    }

    #[tokio::test]
    async fn a_new_repository_elsewhere_or_in_another_projet_or_already_there_is_refused() {
        let f = mia_in_p();
        let mia = person("mia");
        let err = f
            .workspace
            .create_project_repository(&mia, "p", Some("/home/mia"), "x", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)), "{err:?}");
        let err = f
            .workspace
            .create_project_repository(&mia, "s", None, "x", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)), "{err:?}");
        assert!(f.git.created().is_empty());
        let err = f
            .workspace
            .create_project_repository(&mia, "p", None, "taken", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)), "{err:?}");
        assert!(members_of(&f, "p").is_empty());
        // An existing repository in no Projet: only who reads the disk takes it.
        f.git.reuse("/clones/loose");
        let err = f
            .workspace
            .create_project_repository(&mia, "p", None, "loose", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)), "{err:?}");
        assert!(members_of(&f, "p").is_empty());
        assert_eq!(
            f.workspace
                .create_project_repository(&mia, "nope", None, "x", &[])
                .await
                .unwrap_err(),
            WorkspaceError::NotFound
        );
    }
}
