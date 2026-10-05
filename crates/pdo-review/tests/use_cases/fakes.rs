//! In-memory adapters for every port of the review: a Run's log projected by
//! the run context's own fold, a git that knows the refs a test names, the
//! instance setting, and an extension host that can refuse.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_review::application::ports::*;
use pdo_review::domain::diff::{FileDiff, StructuredDiff};
use pdo_review::{Review, ReviewPorts};
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::projection::project;
use pdo_run::domain::state::RunState;

/// The Runs' logs, the managers started and the messages handed over.
#[derive(Default)]
pub struct Runs {
    pub logs: Mutex<HashMap<String, Vec<Event>>>,
    pub unreadable: Mutex<bool>,
    pub worktree: Mutex<Option<PathBuf>>,
    pub manager_running: Mutex<bool>,
    pub manager_refusal: Mutex<Option<(u16, serde_json::Value)>>,
    pub handed: Mutex<Vec<(String, String, bool)>>,
    /// The Projets of each Run; none for a Run absent from it.
    pub projects: Mutex<HashMap<String, Vec<String>>>,
}

impl ReviewRuns for Runs {
    fn load<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<Option<ReviewedRun>, String>> {
        Box::pin(async move {
            if *self.unreadable.lock().unwrap() {
                return Err("database is locked".to_string());
            }
            let events = self
                .logs
                .lock()
                .unwrap()
                .get(run_id)
                .cloned()
                .unwrap_or_default();
            Ok(project(&events).map(|state| ReviewedRun {
                events,
                state,
                repo: PathBuf::from("/repo"),
                worktree: self.worktree.lock().unwrap().clone(),
            }))
        })
    }

    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let mut event = event.clone();
            let mut logs = self.logs.lock().unwrap();
            let log = logs.entry(event.run_id.clone()).or_default();
            event.id = Some(log.len() as i64 + 1);
            log.push(event);
            Ok(())
        })
    }

    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async move {
            self.projects
                .lock()
                .unwrap()
                .get(run_id)
                .cloned()
                .unwrap_or_default()
        })
    }

    fn ensure_manager<'a>(
        &'a self,
        _run_id: &'a str,
        _run: &'a RunState,
    ) -> BoxFuture<'a, Result<bool, (u16, serde_json::Value)>> {
        Box::pin(async move {
            if let Some(refusal) = self.manager_refusal.lock().unwrap().clone() {
                return Err(refusal);
            }
            let mut running = self.manager_running.lock().unwrap();
            let created = !*running;
            *running = true;
            Ok(created)
        })
    }

    fn hand_to_manager(&self, run_id: &str, message: String, just_started: bool) {
        self.handed
            .lock()
            .unwrap()
            .push((run_id.to_string(), message, just_started));
    }
}

impl Runs {
    /// The kinds appended to `run_id`, in order.
    pub fn kinds(&self, run_id: &str) -> Vec<EventKind> {
        self.logs
            .lock()
            .unwrap()
            .get(run_id)
            .map(|log| log.iter().map(|e| e.kind.clone()).collect())
            .unwrap_or_default()
    }
}

/// A git whose refs are the ones a test names; every diff is empty.
#[derive(Default)]
pub struct Git {
    pub refs: Mutex<HashMap<String, String>>,
    pub files: Mutex<HashMap<(String, String), String>>,
    pub calls: Mutex<Vec<String>>,
    pub unknown: Mutex<HashSet<String>>,
}

impl Git {
    pub fn with_ref(&self, name: &str, sha: &str) {
        self.refs
            .lock()
            .unwrap()
            .insert(name.to_string(), sha.to_string());
    }

    fn call(&self, what: String) {
        self.calls.lock().unwrap().push(what);
    }
}

impl ReviewGit for Git {
    fn raw_diff(&self, _repo: &Path, range: &str) -> Result<RawDiff, String> {
        self.call(format!("raw {range}"));
        Ok(RawDiff {
            success: true,
            stdout: b"diff --git a/x b/x\n".to_vec(),
            stderr: String::new(),
        })
    }

    fn structured(
        &self,
        _repo: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure> {
        self.call(format!("structured {from} {to} {three_dot}"));
        if self.unknown.lock().unwrap().contains(to) {
            return Err(GitFailure {
                unknown_revision: true,
                message: "git diff failed: unknown revision".into(),
            });
        }
        Ok(StructuredDiff::of(from, to, None, None, three_dot, vec![]))
    }

    fn structured_worktree(
        &self,
        _worktree: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure> {
        self.call(format!("worktree {from} {to} {three_dot}"));
        Ok(StructuredDiff::of(
            from,
            to,
            None,
            Some("snap".into()),
            three_dot,
            vec![],
        ))
    }

    fn path_diff(
        &self,
        _repo: &Path,
        from: &str,
        to: &str,
        path: &str,
    ) -> Result<Vec<FileDiff>, GitFailure> {
        self.call(format!("path {from} {to} {path}"));
        Ok(vec![])
    }

    fn rev_parse(&self, _repo: &Path, git_ref: &str) -> Option<String> {
        self.refs.lock().unwrap().get(git_ref).cloned()
    }

    fn rev_parse_object(&self, repo: &Path, git_ref: &str) -> Option<String> {
        self.rev_parse(repo, git_ref)
    }

    fn snapshot_worktree(&self, _worktree: &Path) -> Result<String, GitFailure> {
        Ok("snap".into())
    }

    fn file_at_ref(&self, _repo: &Path, git_ref: &str, path: &str) -> Result<Vec<u8>, GitFailure> {
        self.files
            .lock()
            .unwrap()
            .get(&(git_ref.to_string(), path.to_string()))
            .map(|s| s.clone().into_bytes())
            .ok_or(GitFailure {
                unknown_revision: false,
                message: "no such file".into(),
            })
    }
}

/// `review_agent_can_resolve`.
#[derive(Default)]
pub struct Settings(pub Mutex<bool>);

impl ReviewSettings for Settings {
    fn agent_can_resolve(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { *self.0.lock().unwrap() })
    }
}

/// An extension host that records every Operation it is asked about, and
/// refuses them all once told to.
#[derive(Default)]
pub struct Host {
    pub refuse: Mutex<bool>,
    pub asked: Mutex<Vec<(Actor, Operation)>>,
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }
    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.asked
            .lock()
            .unwrap()
            .push((actor.clone(), operation.clone()));
        if *self.refuse.lock().unwrap() {
            Authorization::Deny {
                reason: "the test host refuses everything".into(),
            }
        } else {
            Authorization::Allow
        }
    }
    fn publish(&self, _event: &serde_json::Value) {}
}

/// The context over fresh fakes, and the fakes.
pub struct World {
    pub review: Arc<Review>,
    pub runs: Arc<Runs>,
    pub git: Arc<Git>,
    pub settings: Arc<Settings>,
    pub host: Arc<Host>,
}

impl World {
    pub fn new() -> World {
        let runs = Arc::new(Runs::default());
        let git = Arc::new(Git::default());
        let settings = Arc::new(Settings::default());
        let host = Arc::new(Host::default());
        let review = Review::new(ReviewPorts {
            runs: runs.clone(),
            git: git.clone(),
            settings: settings.clone(),
            extensions: host.clone(),
        });
        World {
            review,
            runs,
            git,
            settings,
            host,
        }
    }

    /// A Run that started from `fork` and completed, its tip on `tip`.
    pub async fn completed_run(&self, run_id: &str) {
        for (kind, payload) in [
            (
                EventKind::RunStarted,
                Some(serde_json::json!({ "pipeline_name": "p", "fork_sha": "f0" })),
            ),
            (EventKind::RunCompleted, None),
        ] {
            self.runs
                .append(&Event {
                    id: None,
                    run_id: run_id.into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind,
                    node_id: None,
                    iter: None,
                    payload,
                })
                .await
                .unwrap();
        }
        self.git.with_ref("f0", "f0f0f0f0f0");
        self.git
            .with_ref(&format!("pdo/run-{run_id}"), "a1a1a1a1a1");
    }

    /// The operations the host was asked about, as `(resource action, target)`.
    pub fn asked(&self) -> Vec<String> {
        self.host
            .asked
            .lock()
            .unwrap()
            .iter()
            .map(|(_, op)| format!("{op:?}"))
            .collect()
    }

    pub fn refuse(&self) {
        *self.host.refuse.lock().unwrap() = true;
    }
}
