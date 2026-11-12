//! The clones of the daemon (CORE-97): running in the background, and the
//! last ones finished, for a page that was closed while they ran.

use std::sync::Mutex;

use pdo_kernel::Actor;
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
    /// Who asked for it: who started it, and who joined it while it ran.
    requested_by: Vec<Actor>,
    /// Fires the cancel of a running clone.
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    /// The fingerprint of the credentials it runs with
    /// (`GitCredentials::fingerprint`).
    credentials: u64,
}

/// What a clone into a folder finds there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Join {
    /// No clone runs into that folder.
    Free,
    /// The running clone, now also for the Projets asked.
    Joined(CloneJob),
    /// A clone runs into that folder with other credentials: joining it would
    /// add what they reach to Projets of an Actor who may not reach it.
    Busy(CloneJob),
}

/// A clone, and who asked for it (never on the wire: a clone shows to who
/// asked for it, [`Workspace::clone_jobs`](super::Workspace::clone_jobs)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tracked {
    pub job: CloneJob,
    pub requested_by: Vec<Actor>,
}

impl Entry {
    fn tracked(&self) -> Tracked {
        Tracked {
            job: self.job.clone(),
            requested_by: self.requested_by.clone(),
        }
    }

    /// `actor` asked for it too, for `project_ids`.
    fn asked_by(&mut self, actor: &Actor, project_ids: impl IntoIterator<Item = String>) {
        for id in project_ids {
            if !self.job.project_ids.contains(&id) {
                self.job.project_ids.push(id);
            }
        }
        if !self.requested_by.contains(actor) {
            self.requested_by.push(actor.clone());
        }
    }
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

    pub(super) fn list(&self) -> Vec<Tracked> {
        self.lock().iter().rev().map(Entry::tracked).collect()
    }

    pub(super) fn get(&self, id: &str) -> Option<Tracked> {
        self.lock()
            .iter()
            .find(|e| e.job.id == id)
            .map(Entry::tracked)
    }

    /// The running clone into `path`, now also asked by `actor` for
    /// `project_ids` when it runs with the same `credentials`.
    pub(super) fn join(
        &self,
        path: &str,
        actor: &Actor,
        project_ids: &[String],
        credentials: u64,
    ) -> Join {
        joined(&mut self.lock(), path, actor, project_ids, credentials)
    }

    /// Records a running clone asked by `actor`, unless one into the same
    /// folder runs already (see [`Self::join`]). One lock for both, so two
    /// asks at once never start two clones of one folder.
    pub(super) fn start(
        &self,
        job: CloneJob,
        actor: &Actor,
        credentials: u64,
        cancel: tokio::sync::oneshot::Sender<()>,
    ) -> Join {
        let mut entries = self.lock();
        let found = joined(&mut entries, &job.path, actor, &job.project_ids, credentials);
        if found == Join::Free {
            push(&mut entries, job, actor, Some(cancel), credentials);
        }
        found
    }

    /// Records a finished clone asked by `actor`.
    pub(super) fn push(&self, job: CloneJob, actor: &Actor) {
        push(&mut self.lock(), job, actor, None, 0);
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

    /// Cancels a running clone; a finished one leaves the list (dismissed).
    pub(super) fn cancel(&self, id: &str) -> Option<CloneJob> {
        let mut entries = self.lock();
        let at = entries.iter().position(|e| e.job.id == id)?;
        if entries[at].job.state != CloneState::Running {
            return Some(entries.remove(at).job);
        }
        if let Some(cancel) = entries[at].cancel.take() {
            let _ = cancel.send(());
        }
        Some(entries[at].job.clone())
    }
}

/// What a clone into `path` with `credentials`, asked by `actor`, finds in
/// `entries`.
fn joined(
    entries: &mut [Entry],
    path: &str,
    actor: &Actor,
    project_ids: &[String],
    credentials: u64,
) -> Join {
    let Some(entry) = entries
        .iter_mut()
        .find(|e| e.job.state == CloneState::Running && e.job.path == path)
    else {
        return Join::Free;
    };
    if entry.credentials != credentials {
        return Join::Busy(entry.job.clone());
    }
    entry.asked_by(actor, project_ids.iter().cloned());
    Join::Joined(entry.job.clone())
}

/// Adds `job` asked by `actor`, dropping the oldest finished clones beyond
/// [`FINISHED_KEPT`].
fn push(
    entries: &mut Vec<Entry>,
    job: CloneJob,
    actor: &Actor,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    credentials: u64,
) {
    entries.push(Entry {
        job,
        requested_by: vec![actor.clone()],
        cancel,
        credentials,
    });
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

    fn person(id: &str) -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: id.into(),
        }
    }

    #[test]
    fn a_second_start_of_one_folder_joins_the_first() {
        let jobs = CloneJobs::default();
        let (alice, bob) = (person("alice"), person("bob"));
        let (tx, _rx) = tokio::sync::oneshot::channel();
        assert_eq!(jobs.start(job("a", "p1"), &alice, 7, tx), Join::Free);
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let Join::Joined(running) = jobs.start(job("b", "p2"), &bob, 7, tx) else {
            panic!("joined");
        };
        assert_eq!(running.id, "a");
        assert_eq!(
            running.project_ids,
            vec!["p1".to_string(), "p2".to_string()]
        );
        let listed = jobs.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].requested_by, vec![alice.clone(), bob.clone()]);
        // Joined again by who asked already: named once.
        let again = jobs.join("/clones/app", &alice, &["p3".to_string()], 7);
        assert!(matches!(again, Join::Joined(_)));
        let tracked = jobs.get("a").unwrap();
        assert_eq!(tracked.requested_by, vec![alice, bob]);
        assert_eq!(tracked.job.project_ids, ["p1", "p2", "p3"]);
    }

    #[test]
    fn a_clone_with_other_credentials_is_never_joined() {
        let jobs = CloneJobs::default();
        let (alice, bob) = (person("alice"), person("bob"));
        let (tx, _rx) = tokio::sync::oneshot::channel();
        assert_eq!(jobs.start(job("a", "p1"), &alice, 7, tx), Join::Free);
        let Join::Busy(running) = jobs.join("/clones/app", &bob, &["p2".to_string()], 8) else {
            panic!("busy");
        };
        assert_eq!(running.project_ids, vec!["p1".to_string()], "p2 not added");
        let (tx, _rx) = tokio::sync::oneshot::channel();
        assert!(matches!(jobs.start(job("b", "p2"), &bob, 8, tx), Join::Busy(_)));
        let listed = jobs.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].requested_by, vec![alice], "bob did not join it");
    }

    #[test]
    fn cancelling_a_finished_clone_dismisses_it() {
        let jobs = CloneJobs::default();
        let mut failed = job("a", "p");
        failed.state = CloneState::Failed;
        let actor = Actor::LocalOperator;
        jobs.push(failed, &actor);
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let mut running = job("b", "p");
        running.path = "/clones/other".into();
        assert_eq!(jobs.start(running, &actor, 0, tx), Join::Free);

        assert_eq!(jobs.cancel("a").unwrap().state, CloneState::Failed);
        assert_eq!(jobs.get("a"), None);

        assert_eq!(jobs.cancel("b").unwrap().state, CloneState::Running);
        assert!(rx.try_recv().is_ok());
        assert!(jobs.get("b").is_some());
    }

    #[test]
    fn only_the_last_finished_clones_are_kept() {
        let jobs = CloneJobs::default();
        for i in 0..FINISHED_KEPT + 5 {
            let mut done = job(&i.to_string(), "p");
            done.state = CloneState::Done;
            jobs.push(done, &Actor::LocalOperator);
        }
        let listed = jobs.list();
        assert_eq!(listed.len(), FINISHED_KEPT);
        assert_eq!(listed[0].job.id, (FINISHED_KEPT + 4).to_string());
    }
}
