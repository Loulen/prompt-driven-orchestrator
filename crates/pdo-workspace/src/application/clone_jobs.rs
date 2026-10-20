//! The clones of the daemon (CORE-97): running in the background, and the
//! last ones finished, for a page that was closed while they ran.

use std::sync::Mutex;

use serde::Serialize;
use ts_rs::TS;

use crate::domain::clone::CloneProgress;

/// How many finished clones the list keeps, newest first, for the pages that
/// were closed while they ran.
const FINISHED_KEPT: usize = 20;

/// Where a clone is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CloneState {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// A clone, as `GET /repos/clones` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CloneJob {
    pub id: String,
    /// The URL, its credentials dropped.
    pub url: String,
    /// Where the repository is (or will be), absolute.
    pub path: String,
    /// The Projets the repository joins once cloned.
    pub project_ids: Vec<String>,
    pub state: CloneState,
    /// The URL was already cloned there: nothing was cloned again.
    pub reused: bool,
    pub progress: Option<CloneProgress>,
    /// Why it failed, as a sentence to show.
    pub error: Option<String>,
}

struct Entry {
    job: CloneJob,
    /// Fires the cancel of a running clone.
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

/// The clones of the daemon, running and recently finished. In memory: a
/// clone does not outlive the daemon that runs it.
#[derive(Default)]
pub struct CloneJobs {
    entries: Mutex<Vec<Entry>>,
}

impl CloneJobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(super) fn list(&self) -> Vec<CloneJob> {
        self.lock().iter().rev().map(|e| e.job.clone()).collect()
    }

    pub(super) fn get(&self, id: &str) -> Option<CloneJob> {
        self.lock()
            .iter()
            .find(|e| e.job.id == id)
            .map(|e| e.job.clone())
    }

    /// The running clone into `path`, now also for `project_id`.
    pub(super) fn join(&self, path: &str, project_id: Option<&str>) -> Option<CloneJob> {
        let mut entries = self.lock();
        let entry = entries
            .iter_mut()
            .find(|e| e.job.state == CloneState::Running && e.job.path == path)?;
        if let Some(id) = project_id {
            if !entry.job.project_ids.iter().any(|p| p == id) {
                entry.job.project_ids.push(id.to_string());
            }
        }
        Some(entry.job.clone())
    }

    pub(super) fn push(&self, job: CloneJob, cancel: Option<tokio::sync::oneshot::Sender<()>>) {
        let mut entries = self.lock();
        entries.push(Entry { job, cancel });
        let finished = entries
            .iter()
            .filter(|e| e.job.state != CloneState::Running)
            .count();
        if finished > FINISHED_KEPT {
            let mut drop = finished - FINISHED_KEPT;
            entries.retain(|e| {
                if drop > 0 && e.job.state != CloneState::Running {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    pub(super) fn update(&self, id: &str, change: impl FnOnce(&mut CloneJob)) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.job.id == id) {
            change(&mut entry.job);
        }
    }

    /// Ends a running clone; the Projets it joins, read at that moment.
    pub(super) fn finish(&self, id: &str, state: CloneState, error: Option<String>) -> Vec<String> {
        let mut entries = self.lock();
        let Some(entry) = entries.iter_mut().find(|e| e.job.id == id) else {
            return Vec::new();
        };
        if entry.job.state == CloneState::Running {
            entry.job.state = state;
            entry.job.error = error;
        }
        entry.cancel = None;
        entry.job.project_ids.clone()
    }

    pub(super) fn cancel(&self, id: &str) -> Option<CloneJob> {
        let mut entries = self.lock();
        let entry = entries.iter_mut().find(|e| e.job.id == id)?;
        if let Some(cancel) = entry.cancel.take() {
            let _ = cancel.send(());
        }
        Some(entry.job.clone())
    }
}
