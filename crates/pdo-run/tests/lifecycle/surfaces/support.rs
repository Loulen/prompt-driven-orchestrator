//! The run context the Run's surfaces are tested over: the lifecycle harness
//! reading a Run's detail from disk as the daemon does, the same names the
//! tests were written against, and their git and `$HOME` fixtures.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use pdo_session::application::ports::Tmux;
use tower::ServiceExt;

use crate::harness::{build_router, build_with, event_log, Build, TestState};

/// A run context over a fresh temporary repository.
pub async fn test_state() -> Arc<TestState> {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().to_path_buf();
    build_with(dir, Some(root), on_disk()).await
}

/// A run context over `dir`, the test's own repository.
pub async fn test_state_with_dir(dir: &Path) -> Arc<TestState> {
    test_state_with_dir_and_docker(dir, None).await
}

/// Like [`test_state_with_dir`], with `docker` standing for the docker binary.
pub async fn test_state_with_dir_and_docker(
    dir: &Path,
    docker_cmd_override: Option<String>,
) -> Arc<TestState> {
    build_with(
        dir.to_path_buf(),
        None,
        Build {
            docker_cmd_override,
            ..on_disk()
        },
    )
    .await
}

/// A run context whose tmux says every session is alive.
pub async fn test_state_with_alive_sessions() -> Arc<TestState> {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().to_path_buf();
    build_with(
        dir,
        Some(root),
        Build {
            tmux: Some(Arc::new(AliveTmux)),
            ..on_disk()
        },
    )
    .await
}

fn on_disk() -> Build {
    Build {
        on_disk: true,
        ..Build::default()
    }
}

/// Archive `run_id` and its terminated descendants, as `cleanup_run` does.
pub async fn cleanup_run(state: &TestState, run_id: &str) -> Response {
    pdo_run::application::archive::cleanup_run(&state.run, run_id).await
}

/// Archive `run_id`, then start its twin, as `retry_all` does.
pub async fn retry_all_run(state: &TestState, run_id: &str) -> Response {
    pdo_run::application::retry_all::retry_all_run(&state.run, run_id).await
}

/// A Run's own repository, or the daemon's when it names none.
pub fn effective_repo_root(state: &TestState, run_state: &event_log::RunState) -> PathBuf {
    pdo_run::application::paths::effective_repo_root(&state.run, run_state)
}

/// A Run's events and its projection, `None` when it has no log.
pub async fn reload_run_state(
    state: &TestState,
    run_id: &str,
) -> Option<(Vec<event_log::Event>, event_log::RunState)> {
    pdo_run::application::events::reload_run_state(&state.run, run_id).await
}

/// Seed a run as `RunStarted` followed by `extra` run-level events (e.g.
/// `RunCompleted`, `RunFailed`, `RunArchived`), with no nodes spawned. Lets
/// the `/runs/reapable` tests construct a run in any terminal/live status.
pub async fn seed_run_with_run_events(
    state: &Arc<TestState>,
    run_id: &str,
    pipeline_name: &str,
    extra: &[event_log::EventKind],
) {
    let mut events = vec![event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
    }];
    for kind in extra {
        events.push(event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: kind.clone(),
            node_id: None,
            iter: None,
            payload: None,
        });
    }
    for ev in &events {
        crate::harness::append_event(state, ev).await.unwrap();
    }
}

/// Create the on-disk worktree dir the daemon would have forked for a run,
/// so the `/runs/reapable` `worktree_present` check sees it.
pub fn mk_run_worktree_dir(repo_root: &Path, run_id: &str) {
    std::fs::create_dir_all(
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree"),
    )
    .unwrap();
}

pub fn run_git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A `git clone` of a freshly-seeded origin, so `work` carries real
/// remote-tracking refs. Returns `(origin, work)`; keep both alive so the refs
/// keep resolving.
pub fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let origin = tempfile::tempdir().unwrap();
    run_git(origin.path(), &["init", "-b", "main"]);
    run_git(origin.path(), &["config", "user.email", "test@test.com"]);
    run_git(origin.path(), &["config", "user.name", "Test"]);
    std::fs::write(origin.path().join("README.md"), "# test\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "init"]);
    run_git(origin.path(), &["checkout", "-b", "feature-remote-only"]);
    std::fs::write(origin.path().join("x.txt"), "x\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "feat"]);
    run_git(origin.path(), &["checkout", "main"]);

    let work = tempfile::tempdir().unwrap();
    run_git(
        origin.path(),
        &[
            "clone",
            "--quiet",
            origin.path().to_str().unwrap(),
            work.path().to_str().unwrap(),
        ],
    );
    run_git(work.path(), &["config", "user.email", "test@test.com"]);
    run_git(work.path(), &["config", "user.name", "Test"]);
    (origin, work)
}

/// Add `count` commits on `dir`'s current branch.
pub fn commit_n(dir: &Path, count: usize, prefix: &str) {
    for i in 0..count {
        let file = dir.join(format!("{prefix}-{i}.txt"));
        std::fs::write(&file, format!("{prefix} {i}\n")).unwrap();
        run_git(dir, &["add", "."]);
        run_git(dir, &["commit", "-m", &format!("{prefix} {i}")]);
    }
}

/// Serializes the tests that point `$HOME` at a temporary directory.
static HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard that sets HOME to a temporary directory and restores it on drop.
/// Holds [`HOME_TEST_LOCK`] to prevent concurrent env var mutation.
pub struct FakeHome {
    _lock: std::sync::MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
    prev: Option<String>,
}

impl FakeHome {
    pub fn new() -> Self {
        // Poison-tolerant: the lock only serializes HOME mutation; a panic in
        // another test must not cascade here.
        let lock = HOME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("HOME").ok();
        std::env::set_var("HOME", dir.path());
        Self {
            _lock: lock,
            dir,
            prev,
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for FakeHome {
    fn drop(&mut self) {
        if let Some(ref h) = self.prev {
            std::env::set_var("HOME", h);
        }
    }
}

/// A tmux for the tests of the run side: every session "exists", nothing runs.
pub struct AliveTmux;

impl Tmux for AliveTmux {
    fn socket(&self) -> &str {
        "pdo-test-alive"
    }
    fn session_path(&self) -> String {
        String::new()
    }
    fn new_session(
        &self,
        _name: &str,
        _working_dir: &Path,
        _script: &str,
        _what: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn kill(&self, _name: &str) {}
    fn session_exists(&self, _name: &str) -> bool {
        true
    }
    fn server_alive(&self) -> Option<bool> {
        Some(true)
    }
    fn list_pdo_sessions(&self) -> std::collections::HashSet<String> {
        Default::default()
    }
    fn is_attached(&self, _name: &str) -> bool {
        false
    }
    fn capture(&self, _name: &str) -> Option<String> {
        None
    }
    fn send_keys(&self, _name: &str, _text: &str) {}
    fn paste(&self, _name: &str, _text: &str) {}
    fn paste_without_enter(&self, _name: &str, _text: &str) -> bool {
        false
    }
    fn press_enter(&self, _name: &str) -> bool {
        false
    }
    fn attach_argv(&self, _name: &str, _ignore_size: bool) -> Vec<String> {
        Vec::new()
    }
    fn attach_command(&self, _name: &str) -> String {
        String::new()
    }
    fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
        false
    }
}

/// Shared POST-cleanup helper for the #315 tests. Returns the HTTP status so
/// callers assert the archive succeeded.
pub async fn post_cleanup_run(state: &Arc<TestState>, run_id: &str) -> StatusCode {
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}
