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

    /// Records a running clone, unless one into the same folder runs already:
    /// that one, then also for the Projets of `job`, is returned. One lock
    /// for both, so two asks at once never start two clones of one folder.
    pub(super) fn start(
        &self,
        job: CloneJob,
        cancel: tokio::sync::oneshot::Sender<()>,
    ) -> Option<CloneJob> {
        let mut entries = self.lock();
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.job.state == CloneState::Running && e.job.path == job.path)
        {
            for id in job.project_ids {
                if !entry.job.project_ids.contains(&id) {
                    entry.job.project_ids.push(id);
                }
            }
            return Some(entry.job.clone());
        }
        push(&mut entries, job, Some(cancel));
        None
    }

    pub(super) fn push(&self, job: CloneJob, cancel: Option<tokio::sync::oneshot::Sender<()>>) {
        push(&mut self.lock(), job, cancel);
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

/// Adds `job`, dropping the oldest finished clones beyond [`FINISHED_KEPT`].
fn push(entries: &mut Vec<Entry>, job: CloneJob, cancel: Option<tokio::sync::oneshot::Sender<()>>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, project: &str) -> CloneJob {
        CloneJob {
            id: id.into(),
            url: "https://h/acme/app".into(),
            path: "/clones/app".into(),
            project_ids: vec![project.into()],
            state: CloneState::Running,
            reused: false,
            progress: None,
            error: None,
        }
    }

    #[test]
    fn a_second_start_of_one_folder_joins_the_first() {
        let jobs = CloneJobs::default();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        assert_eq!(jobs.start(job("a", "p1"), tx), None);
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let running = jobs.start(job("b", "p2"), tx).unwrap();
        assert_eq!(running.id, "a");
        assert_eq!(
            running.project_ids,
            vec!["p1".to_string(), "p2".to_string()]
        );
        assert_eq!(jobs.list().len(), 1);
    }

    #[test]
    fn only_the_last_finished_clones_are_kept() {
        let jobs = CloneJobs::default();
        for i in 0..FINISHED_KEPT + 5 {
            let mut done = job(&i.to_string(), "p");
            done.state = CloneState::Done;
            jobs.push(done, None);
        }
        let listed = jobs.list();
        assert_eq!(listed.len(), FINISHED_KEPT);
        assert_eq!(listed[0].id, (FINISHED_KEPT + 4).to_string());
    }
}
