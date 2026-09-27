//! The run context over its real adapters — the SQLite event store on an
//! in-memory database, tmux on a socket of its own, git and the filesystem in a
//! temporary repository — and in-memory adapters for the ports the other
//! contexts answer (settings, Projets, skills, the Run's legacy surfaces).
//! Driven through its router, as the local operator.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::event_bus::BroadcastEventBus;
use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::Actor;
use pdo_run::adapters::manager_sessions::TmuxManagerSessions;
use pdo_run::adapters::sqlite_event_store::{self, SqliteEventStore};
use pdo_run::application::ports::*;
use pdo_run::domain::event::Event;
use pdo_run::domain::state::RunState;
use pdo_run::{Runs, RunsOptions, RunsPorts};
use pdo_sandbox::application::ports::{SandboxProfileRepository, StoreError};
use pdo_sandbox::domain::profile::draft::ProfileDraft;
use pdo_sandbox::domain::profile::ProfileDiff;
use pdo_session::application::ports::Tmux;
use pdo_skill::domain::delivery::SkippedSkill;
use pdo_skill::domain::selection::ActiveSkill;
use pdo_workspace::domain::project::Project;
use tokio::sync::broadcast;

/// A Run's events, its state and their projection, under the name these tests
/// were written against.
pub mod event_log {
    pub use pdo_run::domain::event::*;
    pub use pdo_run::domain::projection::*;
    pub use pdo_run::domain::state::*;
}

/// The run context of one test, and what it runs over.
pub struct TestState {
    pub db: sqlx::SqlitePool,
    pub run: Arc<Runs>,
    _root: Option<tempfile::TempDir>,
}

/// A unique tmux socket per test (`pdo-<port>`), so one test's session exit
/// never tears down the server another test is spawning on.
pub fn next_test_daemon_port() -> u16 {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let base = 20_000 + (std::process::id() % 20_000) as u16;
    base.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Nothing a Run's detail reads from disk.
struct NoLiveFacts;
impl LiveFacts for NoLiveFacts {
    fn fill<'a>(&'a self, _run: &'a mut RunState, _events: &'a [Event]) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

/// The instance settings of a fresh instance: nothing stored.
struct DefaultSettings;
impl InstanceSettings for DefaultSettings {
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>> {
        Box::pin(async { Some(InstanceDefaults::default()) })
    }
    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>> {
        Box::pin(async { BTreeMap::new() })
    }
}

/// The Run's legacy surfaces, which these tests never reach.
struct NoSurfaces;
impl RunSurfaces for NoSurfaces {
    fn cleanup<'a>(&'a self, _run_id: &'a str) -> BoxFuture<'a, Response> {
        Box::pin(async {
            (axum::http::StatusCode::NOT_IMPLEMENTED, "no archive here").into_response()
        })
    }
    fn retry_all<'a>(&'a self, _run_id: &'a str) -> BoxFuture<'a, Response> {
        Box::pin(async {
            (axum::http::StatusCode::NOT_IMPLEMENTED, "no create here").into_response()
        })
    }
}

/// No Projet owns anything.
struct NoProjects;
impl Projects for NoProjects {
    fn owner_of<'a>(&'a self, _path: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>> {
        Box::pin(async { Ok(None) })
    }
}

/// An empty bank: nothing to deliver.
struct NoSkills;
impl NodeSkills for NoSkills {
    fn bank_names(&self) -> BoxFuture<'_, BTreeMap<String, String>> {
        Box::pin(async { BTreeMap::new() })
    }
    fn deliver(
        &self,
        _worktree: &Path,
        _run_repo_root: &Path,
        _run_id: &str,
        _skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>> {
        Ok(Vec::new())
    }
}

/// The declared wait, which these tests never declare.
struct NoWaits;
impl DeclaredWaits for NoWaits {
    fn on_completion_not_released<'a>(
        &'a self,
        _run_id: &'a str,
        _node_id: &'a str,
        _iter: i64,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

/// No staging profile was ever edited.
struct NoProfiles;
impl SandboxProfileRepository for NoProfiles {
    fn get<'a>(
        &'a self,
        _name: &'a str,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<Option<ProfileDiff>, StoreError>>
    {
        Box::pin(async { Ok(None) })
    }
    fn list(
        &self,
    ) -> pdo_sandbox::application::ports::BoxFuture<'_, Result<Vec<ProfileDiff>, StoreError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn upsert<'a>(
        &'a self,
        _name: &'a str,
        _profile: &'a ProfileDraft,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<ProfileDiff, StoreError>> {
        Box::pin(async { Err(StoreError("read-only in the tests".into())) })
    }
    fn delete<'a>(
        &'a self,
        _name: &'a str,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<bool, StoreError>> {
        Box::pin(async { Ok(false) })
    }
}

async fn build(
    root: PathBuf,
    owned: Option<tempfile::TempDir>,
    tmux: Option<Arc<dyn Tmux>>,
    docker_cmd_override: Option<String>,
) -> Arc<TestState> {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlite_event_store::init(&db).await.unwrap();
    let (bus_tx, _) = broadcast::channel(80);
    let port = next_test_daemon_port();
    let tmux: Arc<dyn Tmux> =
        tmux.unwrap_or_else(|| Arc::new(pdo_session::adapters::tmux::TmuxCli::for_daemon(port)));
    let run = Runs::new(
        RunsPorts {
            store: Arc::new(SqliteEventStore::new(db.clone())),
            bus: Arc::new(BroadcastEventBus::from_sender(bus_tx.clone())),
            live_facts: Arc::new(NoLiveFacts),
            managers: Arc::new(TmuxManagerSessions(tmux.clone())),
            tmux,
            waits: Arc::new(NoWaits),
            projects: Arc::new(NoProjects),
            skills: Arc::new(NoSkills),
            sandbox_profiles: Arc::new(NoProfiles),
            settings: Arc::new(DefaultSettings),
            surfaces: Arc::new(NoSurfaces),
            extensions: Arc::new(NoExtensions),
        },
        RunsOptions {
            daemon_port: port,
            allowed_ws_origins: Vec::new(),
            repo_root: root.clone(),
            // Self-destructing no-op: a spawned node session runs `true` and
            // exits at once — never a real harness, never a lingering session.
            tmux_cmd_override: Some("exec true".to_string()),
            docker_cmd_override,
            sandbox_home_override: None,
            panic_on_stale_sweep: false,
            panic_on_spawn: false,
        },
    );
    Arc::new(TestState {
        db,
        run,
        _root: owned,
    })
}

/// A run context over a fresh temporary repository.
pub async fn test_state() -> Arc<TestState> {
    test_state_with_tmux(None).await
}

pub async fn test_state_with_tmux(tmux: Option<Arc<dyn Tmux>>) -> Arc<TestState> {
    let root = tempfile::tempdir().unwrap();
    build(root.path().to_path_buf(), Some(root), tmux, None).await
}

/// A run context over `dir`, the test's own repository.
pub async fn test_state_with_dir(dir: &Path) -> Arc<TestState> {
    test_state_with_dir_and_docker(dir, None).await
}

pub async fn test_state_with_dir_and_docker(
    dir: &Path,
    docker_cmd_override: Option<String>,
) -> Arc<TestState> {
    build(dir.to_path_buf(), None, None, docker_cmd_override).await
}

/// The context's routes, as the composition root serves them behind
/// `identify` for the local operator.
pub fn build_router(state: Arc<TestState>) -> Router {
    pdo_run::router(state.run.clone()).layer(Extension(Actor::LocalOperator))
}

/// Append `event` to its Run's log, as every in-process caller does.
pub async fn append_event(state: &TestState, event: &Event) -> anyhow::Result<()> {
    pdo_run::application::events::append_event(&state.run, event).await
}

/// Every event of `run_id`, in append order.
pub async fn load_events(db: &sqlx::SqlitePool, run_id: &str) -> anyhow::Result<Vec<Event>> {
    sqlite_event_store::load(db, run_id).await
}
