//! The background half of a clone ([`super::clone_repository`]): it runs
//! the clone (or checks a reused one) with the credentials of the Actor who
//! asked, then the Projets it was asked for receive the repository.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::GitCredentials;

use super::clone_jobs::{CloneJobs, CloneState};
use super::ports::{Git, ProjectEvents, ProjectRepository};
use crate::domain::clone::{clone_failure, parse_progress, CloneFailure};

/// What the background clone needs of the workspace, and the credentials of
/// the Actor who asked for it.
pub(super) struct Running {
    pub(super) git: Arc<dyn Git>,
    pub(super) projects: Arc<dyn ProjectRepository>,
    pub(super) events: Arc<dyn ProjectEvents>,
    pub(super) clones: Arc<CloneJobs>,
    pub(super) credentials: GitCredentials,
}

impl Running {
    /// Reuse the clone at `dest` once `url` answers to the credentials.
    pub(super) async fn reuse(
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

    pub(super) async fn clone_into(
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
pub(super) async fn joined(
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
