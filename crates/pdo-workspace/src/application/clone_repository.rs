//! Add a repository by its URL (CORE-97): PDO clones it in full into the
//! clones folder, in the background, and the Projets it was asked for receive
//! it once it is there. A URL already cloned reuses its clone; a clone of the
//! same folder already running is joined, never started twice.
//!
//! A clone for a Projet is judged in that Projet: creating a repository there
//! and updating it. A folder of the caller's choosing reaches the disk: it
//! asks the disk's read ([`browse_disk`]). Who asked for a clone follows it
//! (reads and cancels it); every clone shows to who may read the disk.
//!
//! The clone runs with the credentials the extensions provide for the Actor
//! who asks, captured with the request, else the daemon's. With extensions, a
//! clone is reused only once the remote answers to those credentials: nobody
//! adds a repository by reusing a clone they could not have made.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdo_kernel::authorization::authorize_read;
use pdo_kernel::extension_host::{GitCredentials, GitOperation, GitOperationKind};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use serde::Serialize;
use ts_rs::TS;

use super::clone_jobs::{CloneJob, CloneJobs, CloneState, Join, Tracked};
use crate::domain::clone::{
    clone_failure, expand_home, is_remote_url, parse_progress, repository_name, same_remote,
    CloneFailure,
};
use crate::domain::remote::shown_remote;

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::{Git, ProjectEvents, ProjectRepository};
use super::read_access::{browse_disk, create_repository_in, read_project};
use super::workspace::Workspace;

/// Where the clone of a URL goes (`GET /repos/clone-target`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CloneTarget {
    /// The repository's name, the folder's.
    pub name: String,
    /// `<clones folder>/<name>`, absolute.
    pub path: String,
    /// That folder already is a clone of the URL: it will be reused.
    pub cloned: bool,
}

/// What a clone is asked for.
#[derive(Debug, Clone, Default)]
pub struct CloneRequest {
    pub url: String,
    /// The folder to clone into; `None` → `<clones folder>/<name>`. `~` stands
    /// for the home.
    pub path: Option<String>,
    /// The Projet the repository joins once cloned.
    pub project_id: Option<String>,
}

impl Workspace {
    /// Where the clone of `url` goes, and whether it is already there: asked
    /// for the Projet `project_id`, by who may create a repository there;
    /// otherwise by who may read the disk.
    pub async fn clone_target(
        &self,
        actor: &Actor,
        url: &str,
        project_id: Option<&str>,
    ) -> WorkspaceResult<CloneTarget> {
        match blank_to_none(project_id) {
            Some(id) => {
                if self.projects.get(id).await?.is_none() {
                    return Err(WorkspaceError::NotFound);
                }
                authorize_read(self.host(), actor, &create_repository_in(id))?;
                let update = Operation::new(ResourceKind::Project, Verb::Update).on(id);
                authorize_read(self.host(), actor, &update.in_projects([id]))?;
            }
            None => authorize_read(self.host(), actor, &browse_disk())?,
        }
        let (name, path) = self.default_destination(url).await?;
        let cloned = self.is_clone_of(&path, url);
        Ok(CloneTarget {
            name,
            path: path.to_string_lossy().into_owned(),
            cloned,
        })
    }

    /// The clones `actor` follows, newest first: those it asked for, or every
    /// one when it may read the disk. Each names only the Projets `actor` may
    /// read (CORE-5).
    pub fn clone_jobs(&self, actor: &Actor) -> WorkspaceResult<Vec<CloneJob>> {
        let everyone = self.authorize_browse(actor).is_ok();
        Ok(self
            .clones
            .list()
            .into_iter()
            .filter_map(|tracked| self.seen_by(actor, everyone, tracked))
            .collect())
    }

    /// One clone `actor` follows; another is not found, as an unknown one.
    pub fn clone_job(&self, actor: &Actor, id: &str) -> WorkspaceResult<CloneJob> {
        let everyone = self.authorize_browse(actor).is_ok();
        self.clones
            .get(id)
            .and_then(|tracked| self.seen_by(actor, everyone, tracked))
            .ok_or(WorkspaceError::NotFound)
    }

    /// `tracked` as `actor` sees it: `None` when it did not ask for it and
    /// may not see `everyone`'s; without the Projets it may not read.
    fn seen_by(&self, actor: &Actor, everyone: bool, tracked: Tracked) -> Option<CloneJob> {
        if !everyone && !tracked.requested_by.contains(actor) {
            return None;
        }
        let mut job = tracked.job;
        job.project_ids
            .retain(|id| authorize_read(self.host(), actor, &read_project(id)).is_ok());
        Some(job)
    }

    /// Stop a running clone `actor` follows; what it wrote is removed. A
    /// finished one leaves the list: its failure, once read, is dismissed.
    /// Another's clone, seen from the disk, also asks to create a repository
    /// in no Projet.
    pub fn cancel_clone(&self, actor: &Actor, id: &str) -> WorkspaceResult<CloneJob> {
        let asked = self
            .clones
            .get(id)
            .is_some_and(|tracked| tracked.requested_by.contains(actor));
        if !asked && self.authorize_browse(actor).is_ok() {
            self.authorize(
                actor,
                Operation::new(ResourceKind::Repository, Verb::Create),
            )?;
        }
        self.clone_job(actor, id)?;
        self.clones.cancel(id).ok_or(WorkspaceError::NotFound)
    }

    /// Clone `request.url`, in the background: the answer is the clone as it
    /// starts (or as it is when one into the same folder runs already). A
    /// folder that already is a clone of the URL is reused at once; one that
    /// holds anything else is refused, by name.
    pub async fn clone_repository(
        &self,
        actor: &Actor,
        request: CloneRequest,
    ) -> WorkspaceResult<CloneJob> {
        let url = request.url.trim().to_string();
        if !is_remote_url(&url) {
            return Err(WorkspaceError::Invalid(format!(
                "not a repository URL: {url}"
            )));
        }
        let project_id = blank_to_none(request.project_id.as_deref()).map(str::to_string);
        match &project_id {
            Some(id) => {
                if self.projects.get(id).await?.is_none() {
                    return Err(WorkspaceError::NotFound);
                }
                self.authorize(actor, create_repository_in(id))?;
                self.authorize(
                    actor,
                    Operation::new(ResourceKind::Project, Verb::Update).on(id),
                )?;
            }
            None => self.authorize(
                actor,
                Operation::new(ResourceKind::Repository, Verb::Create),
            )?,
        }

        let dest = match request
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            Some(typed) => {
                let path = expand_home(typed, self.clones_folder.home().as_deref())
                    .filter(|p| p.is_absolute())
                    .ok_or_else(|| {
                        WorkspaceError::Invalid(format!(
                            "the folder to clone into must be an absolute path: {typed}"
                        ))
                    })?;
                let path = normalized(&path);
                // A folder of the caller's choosing reaches the disk.
                let default = self.default_destination(&url).await.ok().map(|(_, p)| p);
                if default.as_ref() != Some(&path) {
                    self.authorize_browse(actor)?;
                }
                path
            }
            None => self.default_destination(&url).await?.1,
        };
        let path = dest.to_string_lossy().into_owned();
        let project_ids: Vec<String> = project_id.iter().cloned().collect();
        let mut operation = GitOperation::new(actor.clone(), &url, GitOperationKind::Clone);
        if let Some(id) = &project_id {
            operation = operation.in_project(id);
        }
        let credentials = self.host().git_credentials(&operation);
        let fingerprint = credentials.fingerprint();

        match self.clones.join(&path, actor, &project_ids, fingerprint) {
            Join::Free => {}
            Join::Joined(job) => return Ok(job),
            Join::Busy(running) => return Err(busy(&running)),
        }
        let mut job = CloneJob {
            id: uuid::Uuid::new_v4().to_string(),
            url: shown_remote(&url),
            path: path.clone(),
            project_ids,
            state: CloneState::Running,
            reused: false,
            progress: None,
            error: None,
        };

        // A folder of another Projet (its clone, reused, or an empty member)
        // joins only who may read it there.
        if let Some(id) = &project_id {
            self.authorize_taking(actor, &path, id).await?;
        }
        let reuse = self.is_clone_of(&dest, &url);
        job.reused = reuse;
        if reuse && self.host().allows_everything() {
            // No extension: the daemon's one identity made the clone.
            job.state = CloneState::Done;
            joined(&*self.projects, &*self.events, &job.project_ids, &path).await;
            self.clones.push(job.clone(), actor);
            return Ok(job);
        }
        if !reuse && dest.exists() && !is_empty_dir(&dest) {
            return Err(WorkspaceError::Invalid(format!(
                "{path} already exists and is not a clone of {}",
                job.url
            )));
        }

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        // Asked at the same moment as another clone of this folder.
        match self.clones.start(job.clone(), actor, fingerprint, cancel_tx) {
            Join::Free => {}
            Join::Joined(running) => return Ok(running),
            Join::Busy(running) => return Err(busy(&running)),
        }
        let run = Running {
            git: self.git.clone(),
            projects: self.projects.clone(),
            events: self.events.clone(),
            clones: self.clones.clone(),
            credentials,
        };
        let id = job.id.clone();
        if reuse {
            tokio::spawn(async move { run.reuse(id, url, dest, cancel_rx).await });
        } else {
            tokio::spawn(async move { run.clone_into(id, url, dest, cancel_rx).await });
        }
        Ok(job)
    }

    /// `<clones folder>/<name of the URL>`.
    async fn default_destination(&self, url: &str) -> WorkspaceResult<(String, PathBuf)> {
        let url = url.trim();
        if !is_remote_url(url) {
            return Err(WorkspaceError::Invalid(format!(
                "not a repository URL: {url}"
            )));
        }
        let name = repository_name(url).ok_or_else(|| {
            WorkspaceError::Invalid(format!("no repository name in the URL: {url}"))
        })?;
        let folder = self
            .clones_folder
            .folder()
            .await
            .map_err(WorkspaceError::Invalid)?;
        let path = normalized(&folder.join(&name));
        Ok((name, path))
    }

    /// Whether `path` is a repository whose remote is `url`.
    fn is_clone_of(&self, path: &Path, url: &str) -> bool {
        let Ok(repo) = self.git.validate_repository(&path.to_string_lossy()) else {
            return false;
        };
        self.git
            .remote_url(&repo)
            .is_some_and(|remote| same_remote(&remote, url))
    }
}

/// What the background clone needs of the workspace, and the credentials of
/// the Actor who asked for it.
struct Running {
    git: Arc<dyn Git>,
    projects: Arc<dyn ProjectRepository>,
    events: Arc<dyn ProjectEvents>,
    clones: Arc<CloneJobs>,
    credentials: GitCredentials,
}

impl Running {
    /// Reuse the clone at `dest` once `url` answers to the credentials.
    async fn reuse(
        self,
        id: String,
        url: String,
        dest: PathBuf,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) {
        let outcome = tokio::select! {
            biased;
            Ok(()) = cancel => Err(CloneFailure::Cancelled),
            reached = self.git.reach(&url, &self.credentials) => reached,
        };
        let path = dest.to_string_lossy().into_owned();
        match outcome {
            Ok(()) => {
                let projects = self.clones.finish(&id, CloneState::Done, None);
                joined(&*self.projects, &*self.events, &projects, &path).await;
            }
            Err(failure) => self.failed(&id, &url, failure),
        }
    }

    fn failed(&self, id: &str, url: &str, failure: CloneFailure) {
        match failure {
            CloneFailure::Cancelled => {
                self.clones.finish(id, CloneState::Cancelled, None);
            }
            CloneFailure::Failed(stderr) => {
                let error = clone_failure(url, &stderr, &self.credentials);
                tracing::warn!("{error}");
                self.clones.finish(id, CloneState::Failed, Some(error));
            }
        }
    }

    async fn clone_into(
        self,
        id: String,
        url: String,
        dest: PathBuf,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) {
        let clones = self.clones.clone();
        let progress_id = id.clone();
        let on_progress = Box::new(move |line: &str| {
            if let Some(progress) = parse_progress(line) {
                clones.update(&progress_id, |job| job.progress = Some(progress));
            }
        });
        let outcome = self
            .git
            .clone_repository(&url, &dest, &self.credentials, on_progress, cancel)
            .await;
        let path = dest.to_string_lossy().into_owned();
        match outcome {
            Ok(()) => {
                let projects = self.clones.finish(&id, CloneState::Done, None);
                joined(&*self.projects, &*self.events, &projects, &path).await;
            }
            Err(failure) => self.failed(&id, &url, failure),
        }
    }
}

/// `path` joins each of `projects` it is not yet in (the actor was asked when
/// the clone was).
async fn joined(
    projects: &dyn ProjectRepository,
    events: &dyn ProjectEvents,
    ids: &[String],
    path: &str,
) {
    for id in ids {
        match projects.get(id).await {
            Ok(Some(project)) if !project.has_member(path) => {
                if let Err(e) = projects.add_member(id, path).await {
                    tracing::warn!("the clone {path} could not join project {id}: {e}");
                    continue;
                }
                events.project_changed(id).await;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("project {id} cannot be read: {e}"),
        }
    }
}

/// `id` trimmed, `None` when absent or blank.
fn blank_to_none(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

/// The refusal of a clone into a folder another one fills with other
/// credentials: asked again once it ends, it reuses that clone if the remote
/// answers to the asker.
fn busy(running: &CloneJob) -> WorkspaceError {
    WorkspaceError::Invalid(format!(
        "{} is being cloned into {} for someone else: try again once that clone ends",
        running.url, running.path
    ))
}

/// `path` without `.` or `..` segments or a trailing `/`, so two spellings of
/// one folder are one clone.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::{
        alice, denying, fixture, outside_of, person, refusing, ALICE_GIT_ENV,
    };
    use crate::domain::project::Project;

    fn request(url: &str) -> CloneRequest {
        CloneRequest {
            url: url.into(),
            ..CloneRequest::default()
        }
    }

    async fn settled(f: &crate::application::testing::Fixture, id: &str) -> CloneJob {
        settled_for(f, &Actor::LocalOperator, id).await
    }

    async fn settled_for(
        f: &crate::application::testing::Fixture,
        actor: &Actor,
        id: &str,
    ) -> CloneJob {
        for _ in 0..200 {
            let job = f.workspace.clone_job(actor, id).unwrap();
            if job.state != CloneState::Running {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the clone {id} never ended");
    }

    #[tokio::test]
    async fn the_target_is_the_name_under_the_clones_folder() {
        let f = fixture();
        let target = f
            .workspace
            .clone_target(
                &Actor::LocalOperator,
                "https://github.com/acme/analytics.git",
                None,
            )
            .await
            .unwrap();
        assert_eq!(target.name, "analytics");
        assert_eq!(target.path, "/clones/analytics");
        assert!(!target.cloned);
        let err = f
            .workspace
            .clone_target(&Actor::LocalOperator, "/a/path", None)
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)));
    }

    #[tokio::test]
    async fn a_clone_runs_reports_its_progress_and_joins_the_projet() {
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        let job = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://me:tok@example.com/acme/app.git")
                },
            )
            .await
            .unwrap();
        assert_eq!(job.state, CloneState::Running);
        assert_eq!(job.url, "https://example.com/acme/app.git");
        assert_eq!(job.path, "/clones/app");
        let done = settled(&f, &job.id).await;
        assert_eq!(done.state, CloneState::Done);
        assert_eq!(done.progress.unwrap().percent, Some(100));
        assert_eq!(f.git.cloned(), vec![PathBuf::from("/clones/app")]);
        assert_eq!(f.projects.all()[0].members, vec!["/clones/app".to_string()]);
        assert_eq!(f.events.changed(), vec!["p".to_string()]);
    }

    #[tokio::test]
    async fn a_url_already_cloned_is_reused_once_it_answers_the_actor() {
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        // FakeGit: `/repo` is a clone of example.com/acme/repo.
        let job = f
            .workspace
            .clone_repository(
                &alice(),
                CloneRequest {
                    path: Some("/repo".into()),
                    project_id: Some("p".into()),
                    ..request("https://example.com/acme/repo")
                },
            )
            .await
            .unwrap();
        assert!(job.reused);
        let done = settled(&f, &job.id).await;
        assert_eq!((done.state, done.reused), (CloneState::Done, true));
        assert!(f.git.cloned().is_empty());
        assert_eq!(f.git.reached(), vec!["https://example.com/acme/repo"]);
        assert_eq!(
            f.git.credentials()[0]
                .env
                .get(ALICE_GIT_ENV.0)
                .map(String::as_str),
            Some(ALICE_GIT_ENV.1),
            "checked with the credentials provided for the Actor"
        );
        assert_eq!(f.projects.all()[0].members, vec!["/repo".to_string()]);
    }

    #[tokio::test]
    async fn without_extension_a_clone_is_reused_at_once() {
        let f = crate::application::testing::without_extension();
        f.projects.seed(Project::named("p", "Checkout"));
        let job = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    path: Some("/repo".into()),
                    project_id: Some("p".into()),
                    ..request("https://example.com/acme/repo")
                },
            )
            .await
            .unwrap();
        assert_eq!((job.state, job.reused), (CloneState::Done, true));
        assert!(f.git.reached().is_empty() && f.git.cloned().is_empty());
        assert_eq!(f.projects.all()[0].members, vec!["/repo".to_string()]);
    }

    #[tokio::test]
    async fn a_clone_runs_with_the_credentials_provided_for_the_actor() {
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        let job = f
            .workspace
            .clone_repository(
                &alice(),
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://git.example.com/acme/app.git")
                },
            )
            .await
            .unwrap();
        assert_eq!(settled(&f, &job.id).await.state, CloneState::Done);
        let asked = f.host.git_asked();
        assert_eq!(
            (
                &asked[0].actor,
                asked[0].kind,
                asked[0].project.as_deref(),
                asked[0].url.as_str()
            ),
            (
                &alice(),
                GitOperationKind::Clone,
                Some("p"),
                "https://git.example.com/acme/app.git"
            )
        );
        assert_eq!(
            f.git.credentials()[0]
                .env
                .get(ALICE_GIT_ENV.0)
                .map(String::as_str),
            Some(ALICE_GIT_ENV.1)
        );
    }

    #[tokio::test]
    async fn a_refusal_names_whose_credentials() {
        let f = fixture();
        let job = f
            .workspace
            .clone_repository(&alice(), request("https://h/acme/denied.git"))
            .await
            .unwrap();
        let failed = settled(&f, &job.id).await;
        assert!(failed
            .error
            .unwrap()
            .starts_with("The credentials provided for Alice were refused for h: "));

        // A reuse the remote refuses to the Actor adds nothing.
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        let job = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    path: Some("/denied".into()),
                    project_id: Some("p".into()),
                    ..request("https://example.com/acme/denied")
                },
            )
            .await
            .unwrap();
        let failed = settled(&f, &job.id).await;
        assert_eq!(failed.state, CloneState::Failed);
        assert!(failed
            .error
            .unwrap()
            .starts_with("The daemon's git credentials were refused for example.com: "));
        assert!(f.git.cloned().is_empty());
        assert!(f.projects.all()[0].members.is_empty());
    }

    #[tokio::test]
    async fn a_clone_into_a_running_folder_is_joined() {
        let f = fixture();
        f.git.hold_clones();
        let first = f
            .workspace
            .clone_repository(&Actor::LocalOperator, request("https://h/acme/app.git"))
            .await
            .unwrap();
        let second = f
            .workspace
            .clone_repository(&Actor::LocalOperator, request("https://h/acme/app"))
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(
            f.workspace.clone_jobs(&Actor::LocalOperator).unwrap().len(),
            1
        );
        let cancelled = f
            .workspace
            .cancel_clone(&Actor::LocalOperator, &first.id)
            .unwrap();
        assert_eq!(cancelled.id, first.id);
        assert_eq!(settled(&f, &first.id).await.state, CloneState::Cancelled);
    }

    #[tokio::test]
    async fn a_running_clone_is_joined_only_with_the_same_credentials() {
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        f.projects.seed(Project::named("q", "Other"));
        f.git.hold_clones();
        let first = f
            .workspace
            .clone_repository(
                &alice(),
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://h/acme/app.git")
                },
            )
            .await
            .unwrap();
        let err = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    project_id: Some("q".into()),
                    ..request("https://h/acme/app.git")
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&err, WorkspaceError::Invalid(m) if m.contains("for someone else")),
            "{err:?}"
        );
        let job = f.workspace.clone_job(&alice(), &first.id).unwrap();
        assert_eq!(job.project_ids, vec!["p".to_string()], "q never joined");
        let again = f
            .workspace
            .clone_repository(&alice(), request("https://h/acme/app.git"))
            .await
            .unwrap();
        assert_eq!(again.id, first.id, "the same credentials join");
    }

    #[tokio::test]
    async fn a_failure_is_named() {
        let f = fixture();
        let job = f
            .workspace
            .clone_repository(&Actor::LocalOperator, request("https://h/acme/denied.git"))
            .await
            .unwrap();
        let failed = settled(&f, &job.id).await;
        assert_eq!(failed.state, CloneState::Failed);
        assert!(failed
            .error
            .unwrap()
            .starts_with("The daemon's git credentials were refused for h: "));
    }

    #[tokio::test]
    async fn a_path_or_a_folder_or_a_refusal_clones_nothing() {
        let f = fixture();
        let err = f
            .workspace
            .clone_repository(&Actor::LocalOperator, request("~/code/app"))
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)));
        let err = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    path: Some("relative/app".into()),
                    ..request("https://h/acme/app")
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Invalid(_)));
        let err = f
            .workspace
            .clone_repository(
                &Actor::LocalOperator,
                CloneRequest {
                    project_id: Some("nope".into()),
                    ..request("https://h/acme/app")
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err, WorkspaceError::NotFound);

        let f = denying("no");
        let err = f
            .workspace
            .clone_repository(&Actor::LocalOperator, request("https://h/acme/app"))
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)));
        assert!(f.git.cloned().is_empty());
    }

    #[tokio::test]
    async fn a_clone_names_only_the_projets_its_follower_may_read() {
        let f = crate::application::testing::denying_reads_in(&["secret"]);
        f.projects.seed(Project::named("secret", "Secret"));
        f.projects.seed(Project::named("open", "Open"));
        f.git.hold_clones();
        let actor = Actor::LocalOperator;
        for project in ["open", "secret"] {
            f.workspace
                .clone_repository(
                    &actor,
                    CloneRequest {
                        project_id: Some(project.into()),
                        ..request("https://h/acme/shared")
                    },
                )
                .await
                .unwrap();
        }
        let seen = f.workspace.clone_jobs(&actor).unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].project_ids, ["open"]);
    }

    /// The persons may only write in `p` (no disk, no other Projet); the
    /// local operator may do anything.
    fn writer_in_p() -> crate::application::testing::Fixture {
        let f = refusing(|actor, operation| {
            *actor != Actor::LocalOperator && outside_of("p", operation)
        });
        f.projects.seed(Project::named("p", "Payments"));
        f.projects.seed(Project::named("q", "Quotes"));
        f
    }

    #[tokio::test]
    async fn a_clone_for_a_projet_is_judged_in_that_projet() {
        let f = writer_in_p();
        let mia = person("mia");
        let target = f
            .workspace
            .clone_target(&mia, "https://h/acme/app.git", Some("p"))
            .await
            .unwrap();
        assert_eq!(target.path, "/clones/app");
        let job = f
            .workspace
            .clone_repository(
                &mia,
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://h/acme/app.git")
                },
            )
            .await
            .unwrap();
        let asked = f.host.asked();
        assert!(asked.contains(&create_repository_in("p")), "{asked:?}");
        assert_eq!(settled_for(&f, &mia, &job.id).await.state, CloneState::Done);
        let p = f.projects.all().into_iter().find(|p| p.id == "p").unwrap();
        assert_eq!(p.members, ["/clones/app"]);

        // Neither in another Projet, nor in none, nor into a folder of hers.
        for (project, path) in [
            (Some("q"), None),
            (None, None),
            (Some("p"), Some("/elsewhere/app")),
        ] {
            let err = f
                .workspace
                .clone_repository(
                    &mia,
                    CloneRequest {
                        project_id: project.map(str::to_string),
                        path: path.map(str::to_string),
                        ..request("https://h/acme/other.git")
                    },
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, WorkspaceError::Denied(_)),
                "{project:?} {path:?}: {err:?}"
            );
        }
        // Her folder, spelled as the clones folder's own, is no folder of hers.
        let other = f
            .workspace
            .clone_repository(
                &mia,
                CloneRequest {
                    project_id: Some("p".into()),
                    path: Some("/clones/./other".into()),
                    ..request("https://h/acme/other.git")
                },
            )
            .await
            .unwrap();
        settled_for(&f, &mia, &other.id).await;
        assert_eq!(
            f.git.cloned(),
            [PathBuf::from("/clones/app"), PathBuf::from("/clones/other")]
        );
        for project in [Some("q"), None] {
            let err = f
                .workspace
                .clone_target(&mia, "https://h/acme/app.git", project)
                .await
                .unwrap_err();
            assert!(matches!(err, WorkspaceError::Denied(_)), "{project:?}");
        }
    }

    #[tokio::test]
    async fn a_folder_of_another_projet_is_taken_only_by_who_reads_it_there() {
        let f = writer_in_p();
        // FakeGit: `/clones/repo` is a clone of example.com/acme/repo; `s` holds it.
        let mut s = Project::named("s", "Secret");
        s.members = vec!["/clones/repo".into()];
        f.projects.seed(s);
        let mia = person("mia");
        let err = f
            .workspace
            .clone_repository(
                &mia,
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://example.com/acme/repo")
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::Denied(_)), "{err:?}");
        let operator = Actor::LocalOperator;
        let reused = f
            .workspace
            .clone_repository(
                &operator,
                CloneRequest {
                    project_id: Some("p".into()),
                    ..request("https://example.com/acme/repo")
                },
            )
            .await
            .unwrap();
        assert!(reused.reused);
        let asked = f.host.asked();
        let read_in_s = crate::application::read_access::read_repository_in("/clones/repo", "s");
        assert!(asked.contains(&read_in_s), "{asked:?}");
    }

    #[tokio::test]
    async fn a_clone_is_followed_by_who_asked_for_it_and_by_who_reads_the_disk() {
        // Mia and Ivan may only write in `p`; the operator reads the disk.
        let f = writer_in_p();
        f.git.hold_clones();
        let (mia, ivan) = (person("mia"), person("ivan"));
        let in_p = |url: &str| CloneRequest {
            project_id: Some("p".into()),
            ..request(url)
        };
        let hers = f
            .workspace
            .clone_repository(&mia, in_p("https://h/acme/app.git"))
            .await
            .unwrap();
        assert_eq!(f.workspace.clone_job(&mia, &hers.id).unwrap().id, hers.id);
        assert_eq!(f.workspace.clone_jobs(&mia).unwrap().len(), 1);

        // Another person: as an unknown clone.
        assert_eq!(
            f.workspace.clone_job(&ivan, &hers.id).unwrap_err(),
            WorkspaceError::NotFound
        );
        assert_eq!(
            f.workspace.cancel_clone(&ivan, &hers.id).unwrap_err(),
            WorkspaceError::NotFound
        );
        assert!(f.workspace.clone_jobs(&ivan).unwrap().is_empty());

        // Who joins a running clone follows it too.
        let joined = f
            .workspace
            .clone_repository(&ivan, in_p("https://h/acme/app"))
            .await
            .unwrap();
        assert_eq!(joined.id, hers.id);
        assert_eq!(f.workspace.clone_jobs(&ivan).unwrap().len(), 1);

        let operator = Actor::LocalOperator;
        f.workspace
            .clone_repository(&operator, request("https://h/acme/tool.git"))
            .await
            .unwrap();
        assert_eq!(f.workspace.clone_jobs(&operator).unwrap().len(), 2);
        assert_eq!(f.workspace.clone_jobs(&mia).unwrap().len(), 1);

        let cancelled = f.workspace.cancel_clone(&mia, &hers.id).unwrap();
        assert_eq!(cancelled.id, hers.id);
    }

    #[test]
    fn a_path_is_normalized() {
        assert_eq!(normalized(Path::new("/a/./b/../c/")), PathBuf::from("/a/c"));
    }
}
