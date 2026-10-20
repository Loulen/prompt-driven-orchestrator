//! Add a repository by its URL (CORE-97): PDO clones it in full into the
//! clones folder, in the background, and the Projets it was asked for receive
//! it once it is there. A URL already cloned reuses its clone; a clone of the
//! same folder already running is joined, never started twice.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdo_kernel::authorization::authorize_read;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use serde::Serialize;
use ts_rs::TS;

use super::clone_jobs::{CloneJob, CloneJobs, CloneState};
use crate::domain::clone::{
    clone_failure, expand_home, is_remote_url, parse_progress, repository_name, same_remote,
    CloneFailure,
};
use crate::domain::remote::shown_remote;

use super::error::{WorkspaceError, WorkspaceResult};
use super::ports::{Git, ProjectEvents, ProjectRepository};
use super::read_access::read_project;
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
    /// Where the clone of `url` goes, and whether it is already there.
    pub async fn clone_target(&self, actor: &Actor, url: &str) -> WorkspaceResult<CloneTarget> {
        self.authorize(actor, Operation::new(ResourceKind::Repository, Verb::Read))?;
        let (name, path) = self.default_destination(url).await?;
        let cloned = self.is_clone_of(&path, url);
        Ok(CloneTarget {
            name,
            path: path.to_string_lossy().into_owned(),
            cloned,
        })
    }

    /// The clones `actor` may see, newest first: a clone for Projets shows
    /// to who may read one of them (naming those only), as the Projets do
    /// (CORE-5).
    pub fn clone_jobs(&self, actor: &Actor) -> WorkspaceResult<Vec<CloneJob>> {
        self.authorize(actor, Operation::new(ResourceKind::Repository, Verb::Read))?;
        Ok(self
            .clones
            .list()
            .into_iter()
            .filter_map(|job| self.seen_by(actor, job))
            .collect())
    }

    pub fn clone_job(&self, actor: &Actor, id: &str) -> WorkspaceResult<CloneJob> {
        self.authorize(actor, Operation::new(ResourceKind::Repository, Verb::Read))?;
        self.clones
            .get(id)
            .and_then(|job| self.seen_by(actor, job))
            .ok_or(WorkspaceError::NotFound)
    }

    /// `job` as `actor` sees it: without the Projets it may not read; `None`
    /// when it was for Projets only and it may read none of them.
    fn seen_by(&self, actor: &Actor, mut job: CloneJob) -> Option<CloneJob> {
        if job.project_ids.is_empty() {
            return Some(job);
        }
        job.project_ids
            .retain(|id| authorize_read(self.host(), actor, &read_project(id)).is_ok());
        (!job.project_ids.is_empty()).then_some(job)
    }

    /// Stop a running clone; what it wrote is removed. A finished one stays
    /// as it ended.
    pub fn cancel_clone(&self, actor: &Actor, id: &str) -> WorkspaceResult<CloneJob> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Repository, Verb::Create),
        )?;
        self.clones
            .get(id)
            .and_then(|job| self.seen_by(actor, job))
            .ok_or(WorkspaceError::NotFound)?;
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
        self.authorize(
            actor,
            Operation::new(ResourceKind::Repository, Verb::Create),
        )?;
        let project_id = request
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        if let Some(id) = &project_id {
            if self.projects.get(id).await?.is_none() {
                return Err(WorkspaceError::NotFound);
            }
            self.authorize(
                actor,
                Operation::new(ResourceKind::Project, Verb::Update).on(id),
            )?;
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
                normalized(&path)
            }
            None => self.default_destination(&url).await?.1,
        };
        let path = dest.to_string_lossy().into_owned();
        let project_ids: Vec<String> = project_id.iter().cloned().collect();

        if let Some(job) = self.clones.join(&path, project_id.as_deref()) {
            return Ok(job);
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

        if self.is_clone_of(&dest, &url) {
            job.state = CloneState::Done;
            job.reused = true;
            joined(&*self.projects, &*self.events, &job.project_ids, &path).await;
            self.clones.push(job.clone(), None);
            return Ok(job);
        }
        if dest.exists() && !is_empty_dir(&dest) {
            return Err(WorkspaceError::Invalid(format!(
                "{path} already exists and is not a clone of {}",
                job.url
            )));
        }

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        if let Err(running) = self.clones.start(job.clone(), cancel_tx) {
            // Asked at the same moment as another clone of this folder.
            return Ok(running);
        }
        let run = Running {
            git: self.git.clone(),
            projects: self.projects.clone(),
            events: self.events.clone(),
            clones: self.clones.clone(),
        };
        let id = job.id.clone();
        tokio::spawn(async move { run.clone_into(id, url, dest, cancel_rx).await });
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

/// What the background clone needs of the workspace.
struct Running {
    git: Arc<dyn Git>,
    projects: Arc<dyn ProjectRepository>,
    events: Arc<dyn ProjectEvents>,
    clones: Arc<CloneJobs>,
}

impl Running {
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
            .clone_repository(&url, &dest, on_progress, cancel)
            .await;
        let path = dest.to_string_lossy().into_owned();
        match outcome {
            Ok(()) => {
                let projects = self.clones.finish(&id, CloneState::Done, None);
                joined(&*self.projects, &*self.events, &projects, &path).await;
            }
            Err(CloneFailure::Cancelled) => {
                self.clones.finish(&id, CloneState::Cancelled, None);
            }
            Err(CloneFailure::Failed(stderr)) => {
                let error = clone_failure(&url, &stderr);
                tracing::warn!("{error}");
                self.clones.finish(&id, CloneState::Failed, Some(error));
            }
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
    use crate::application::testing::{denying, fixture};
    use crate::domain::project::Project;

    fn request(url: &str) -> CloneRequest {
        CloneRequest {
            url: url.into(),
            ..CloneRequest::default()
        }
    }

    async fn settled(f: &crate::application::testing::Fixture, id: &str) -> CloneJob {
        for _ in 0..200 {
            let job = f.workspace.clone_job(&Actor::LocalOperator, id).unwrap();
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
            )
            .await
            .unwrap();
        assert_eq!(target.name, "analytics");
        assert_eq!(target.path, "/clones/analytics");
        assert!(!target.cloned);
        let err = f
            .workspace
            .clone_target(&Actor::LocalOperator, "/a/path")
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
    async fn a_url_already_cloned_is_reused_without_a_second_clone() {
        let f = fixture();
        f.projects.seed(Project::named("p", "Checkout"));
        // FakeGit: `/repo` is a clone of example.com/acme/repo.
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
        assert!(f.git.cloned().is_empty());
        assert_eq!(f.projects.all()[0].members, vec!["/repo".to_string()]);
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
            .starts_with("The daemon's git credentials were refused"),);
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
    async fn a_clone_for_projets_shows_to_who_may_read_one() {
        let f = crate::application::testing::denying_reads_in(&["secret"]);
        f.projects.seed(Project::named("secret", "Secret"));
        f.projects.seed(Project::named("open", "Open"));
        f.git.hold_clones();
        let actor = Actor::LocalOperator;
        let secret = f
            .workspace
            .clone_repository(
                &actor,
                CloneRequest {
                    project_id: Some("secret".into()),
                    ..request("https://h/acme/hidden")
                },
            )
            .await;
        // Only the reads are refused in « secret »: the clone starts.
        let secret = secret.unwrap();
        let both = f
            .workspace
            .clone_repository(
                &actor,
                CloneRequest {
                    project_id: Some("open".into()),
                    ..request("https://h/acme/shared")
                },
            )
            .await
            .unwrap();
        f.workspace
            .clone_repository(
                &actor,
                CloneRequest {
                    project_id: Some("secret".into()),
                    ..request("https://h/acme/shared")
                },
            )
            .await
            .unwrap();
        let seen = f.workspace.clone_jobs(&actor).unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].id.as_str(), seen[0].project_ids.as_slice()),
            (both.id.as_str(), &["open".to_string()][..])
        );
        assert_eq!(
            f.workspace.clone_job(&actor, &secret.id).unwrap_err(),
            WorkspaceError::NotFound
        );
        assert_eq!(
            f.workspace.cancel_clone(&actor, &secret.id).unwrap_err(),
            WorkspaceError::NotFound
        );
    }

    #[test]
    fn a_path_is_normalized() {
        assert_eq!(normalized(Path::new("/a/./b/../c/")), PathBuf::from("/a/c"));
    }
}
