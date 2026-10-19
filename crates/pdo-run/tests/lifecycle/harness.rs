//! The run context over its real adapters — the SQLite event store on an
//! in-memory database, tmux on a socket of its own, git and the filesystem in a
//! temporary repository — and in-memory adapters for the ports the other
//! contexts answer (settings, Projets, skills, the page mounts). Driven through
//! its router, as the local operator. [`Build::on_disk`] reads a Run's detail
//! from disk too (its LOC, its cost), as the daemon does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use axum::{Extension, Router};
use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::event_bus::BroadcastEventBus;
use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::Actor;
use pdo_pipeline::adapters::self_writes::SelfWrites;
use pdo_run::adapters::disk_live_facts::DiskLiveFacts;
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
use pdo_workspace::domain::provisioning::{ProvisioningRules, ProvisioningScope, ScopedRules};
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
    /// The daemon's repo root: the test's temporary repository.
    pub repo_root: PathBuf,
    /// The page mounts the archives released, in order.
    pub released_pages: Arc<Pages>,
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
    fn cost<'a>(
        &'a self,
        _run: &'a RunState,
        _events: &'a [Event],
    ) -> BoxFuture<'a, Option<pdo_run::domain::state::CostStat>> {
        Box::pin(async { None })
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
    fn profile_named<'a>(
        &'a self,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<Option<ResolvedCombo>, String>> {
        Box::pin(async { Ok(None) })
    }
}

/// The page mounts: none, and every release recorded.
#[derive(Default)]
pub struct Pages(pub std::sync::Mutex<Vec<String>>);
impl RunPages for Pages {
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>> {
        Box::pin(async { Box::new(()) as Box<dyn Send> })
    }
    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(run_id.to_string());
            Ok(())
        })
    }
}

/// No pipeline watcher runs.
struct NoWatcher;
impl RunDirWatch for NoWatcher {
    fn watch(&self, _run_dir: &Path) {}
}

/// No shared pipeline to overwrite.
struct NoSourcePipelines;
impl SourcePipelines for NoSourcePipelines {
    fn exists(&self, _id: &str) -> bool {
        false
    }
    fn fingerprint(&self, _id: &str) -> Option<String> {
        None
    }
    fn overwrite<'a>(
        &'a self,
        _actor: &'a pdo_kernel::Actor,
        _id: &'a str,
        _yaml: &'a str,
        _prompts: &'a std::collections::HashMap<String, String>,
    ) -> pdo_run::application::ports::BoxFuture<'a, Result<(), OverwriteRefusal>> {
        Box::pin(async { Err(OverwriteRefusal::NotFound) })
    }
    fn last_version<'a>(
        &'a self,
        _id: &'a str,
    ) -> pdo_run::application::ports::BoxFuture<
        'a,
        Option<pdo_pipeline::domain::version::PipelineVersionRef>,
    > {
        Box::pin(async { None })
    }
}

/// No Trigger.
struct NoTriggers;
impl PipelineTriggers for NoTriggers {
    fn firing<'a>(
        &'a self,
        _pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<PipelineTrigger>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// An empty bank, as the pipeline context reads it.
struct NoBank;
impl pdo_pipeline::application::ports::SkillBank for NoBank {
    fn sidecar_of(
        &self,
        references: &[pdo_skill::domain::selection::SkillRef],
    ) -> (Vec<pdo_skill::domain::sidecar::SidecarSkill>, Vec<String>) {
        (
            Vec::new(),
            references.iter().map(|r| r.id.clone()).collect(),
        )
    }
    fn import_sidecar<'a>(
        &'a self,
        _actor: &'a Actor,
        _pipeline_name: &'a str,
        _references: Vec<pdo_skill::domain::selection::SkillRef>,
        _sidecar: &'a [pdo_skill::domain::sidecar::SidecarSkill],
    ) -> pdo_pipeline::application::ports::BoxFuture<
        'a,
        Result<
            pdo_skill::domain::sidecar::SidecarImportReport,
            pdo_skill::domain::error::SkillError,
        >,
    > {
        Box::pin(async { Ok(Default::default()) })
    }
}

/// No Projet owns anything, and no tier provisions a Run but its own.
struct NoProjects;
impl Projects for NoProjects {
    fn owner_of<'a>(&'a self, _path: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>> {
        Box::pin(async { Ok(None) })
    }
    fn provisioning_rules<'a>(
        &'a self,
        _repository: &'a str,
        run: ProvisioningRules,
    ) -> BoxFuture<'a, Result<Vec<ScopedRules>, String>> {
        Box::pin(async move {
            Ok(vec![
                ScopedRules {
                    scope: ProvisioningScope::Instance,
                    rules: ProvisioningRules::default(),
                },
                ScopedRules {
                    scope: ProvisioningScope::Project,
                    rules: ProvisioningRules::default(),
                },
                ScopedRules {
                    scope: ProvisioningScope::Run,
                    rules: run,
                },
            ])
        })
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
    fn withdraw(&self, _repo_root: &Path, _run_id: &str) -> anyhow::Result<()> {
        Ok(())
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

/// How one test's run context is built.
#[derive(Default)]
pub struct Build {
    pub tmux: Option<Arc<dyn Tmux>>,
    pub docker_cmd_override: Option<String>,
    /// Replaces `$HOME` for the sandbox staging, the transcripts and the price
    /// files.
    pub home: Option<PathBuf>,
    /// Read a Run's detail from disk ([`DiskLiveFacts`]), as the daemon does.
    pub on_disk: bool,
    /// Over this database instead of a fresh one.
    pub db: Option<sqlx::SqlitePool>,
}

pub async fn build_with(
    root: PathBuf,
    owned: Option<tempfile::TempDir>,
    b: Build,
) -> Arc<TestState> {
    let db = match b.db {
        Some(db) => db,
        None => {
            let db = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            sqlite_event_store::init(&db).await.unwrap();
            db
        }
    };
    let (bus_tx, _) = broadcast::channel(80);
    let port = next_test_daemon_port();
    let tmux: Arc<dyn Tmux> = b
        .tmux
        .unwrap_or_else(|| Arc::new(pdo_session::adapters::tmux::TmuxCli::for_daemon(port)));
    let live_facts: Arc<dyn LiveFacts> = if b.on_disk {
        Arc::new(DiskLiveFacts::new(root.clone(), b.home.clone()))
    } else {
        Arc::new(NoLiveFacts)
    };
    let pages = Arc::new(Pages::default());
    let run = Runs::new(
        RunsPorts {
            store: Arc::new(SqliteEventStore::new(db.clone())),
            bus: Arc::new(BroadcastEventBus::from_sender(bus_tx.clone())),
            live_facts,
            managers: Arc::new(TmuxManagerSessions(tmux.clone())),
            sessions: Arc::new(
                pdo_session::application::session_ledger::SessionLedger::new(
                    tmux.clone(),
                    Arc::new(NoExtensions),
                ),
            ),
            tmux,
            waits: Arc::new(NoWaits),
            projects: Arc::new(NoProjects),
            skills: Arc::new(NoSkills),
            sandbox_profiles: Arc::new(NoProfiles),
            settings: Arc::new(DefaultSettings),
            bank: Arc::new(NoBank),
            pages: pages.clone(),
            run_dirs: Arc::new(NoWatcher),
            self_writes: SelfWrites::new(),
            source_pipelines: Arc::new(NoSourcePipelines),
            pipeline_triggers: Arc::new(NoTriggers),
            extensions: Arc::new(NoExtensions),
        },
        RunsOptions {
            daemon_port: port,
            allowed_ws_origins: Vec::new(),
            repo_root: root.clone(),
            // Self-destructing no-op: a spawned node session runs `true` and
            // exits at once — never a real harness, never a lingering session.
            tmux_cmd_override: Some("exec true".to_string()),
            docker_cmd_override: b.docker_cmd_override,
            sandbox_home_override: b.home,
            panic_on_stale_sweep: false,
            panic_on_spawn: false,
        },
    );
    Arc::new(TestState {
        db,
        run,
        repo_root: root,
        released_pages: pages,
        _root: owned,
    })
}

async fn build(
    root: PathBuf,
    owned: Option<tempfile::TempDir>,
    tmux: Option<Arc<dyn Tmux>>,
    docker_cmd_override: Option<String>,
) -> Arc<TestState> {
    build_with(
        root,
        owned,
        Build {
            tmux,
            docker_cmd_override,
            ..Build::default()
        },
    )
    .await
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
