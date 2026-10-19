//! The wiring of the bounded contexts (ADR-0079): each context's adapters
//! plugged onto its ports, over the resources the boot opened, one file per
//! context. The order follows the context map: a context is wired after the
//! ones it reads. The ones that read a context wired after them (the session
//! context's Run-side ports, the page mounts the Runs release, the shared
//! pipelines a Run overwrites) get it through a [`Late`] handle, bound here
//! once everything is wired.

mod pipeline;
mod platform;
mod review;
mod run;
mod sandbox;
mod session;
mod settings;
pub(crate) mod skill;
mod stats;
mod telemetry;
mod trigger;
mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use pdo_kernel::event_bus::EventBus;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::adapters::self_writes::SelfWrites;
use pdo_platform::domain::install::{InstallMethod, Supervision};
use pdo_platform::domain::release_channel::UpdateChannel;
use pdo_platform::domain::service_health::ServiceHealthOverride;
use pdo_platform::Platform;
use pdo_run::application::ports::RunDirWatch;
use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::application::ports::Tmux;
use pdo_session::Session;
use pdo_settings::adapters::contexts::ProjectTiers;
use pdo_settings::adapters::sqlite::{SqliteAgentProfiles, SqliteInstanceConfig};
use pdo_settings::application::ports::{AgentProfileStore, InstanceConfigStore};
use tokio::sync::broadcast;

use crate::ports::Late;
use crate::spa::static_handler;

/// What the boot opened, and the daemon's knobs the contexts read.
pub struct ContextResources {
    /// The pool of `pdo.db`.
    pub db: sqlx::SqlitePool,
    /// The daemon's repo root (its cwd).
    pub repo_root: PathBuf,
    /// The port the daemon listens on.
    pub port: u16,
    /// The home the update journal lives under: the per-daemon override, else
    /// `$HOME`; `None` when neither is set.
    pub home_root: Option<PathBuf>,
    pub update_source_url: Option<String>,
    /// The distribution's release channel; `None` → the vanilla one.
    pub release_channel: Option<UpdateChannel>,
    pub update_executor_override: Option<String>,
    pub install_method_override: Option<InstallMethod>,
    pub supervision_override: Option<Supervision>,
    pub relaunch_command: Option<Vec<String>>,
    pub service_health_override: Option<ServiceHealthOverride>,
    /// Replaces the harness tail of a session (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
    /// Extra WebSocket origins, parsed at boot.
    pub allowed_ws_origins: Vec<String>,
    /// The kernel's event bus, and its raw channel for the announcements the
    /// contexts still send on it (Projet, pipeline, Trigger, Run-list).
    pub bus: Arc<dyn EventBus>,
    pub bus_tx: broadcast::Sender<serde_json::Value>,
    /// The pipeline watcher a Run's directory is handed to.
    pub run_dirs: Arc<dyn RunDirWatch>,
    /// The daemon's own writes, which the watcher does not announce.
    pub self_writes: SelfWrites,
    pub docker_cmd_override: Option<String>,
    pub sandbox_home_override: Option<PathBuf>,
    /// Debug-only fault injection: the first stale sweep panics.
    pub panic_on_stale_sweep: bool,
    /// Debug-only fault injection: the first spawn panics.
    pub panic_on_spawn: bool,
    /// Debug-only fault injection: when a due Trigger's name matches, its tick
    /// `panic!`s.
    pub panic_on_trigger_name: Option<String>,
    pub price_source_url: Option<String>,
    pub price_refresh_at_boot: bool,
    /// The names of the Projets the extensions read (API 0.11): the
    /// workspace context keeps them current, the boot fills them.
    pub project_names: Arc<pdo_kernel::project_names::ProjectNames>,
}

/// The contexts, wired: the handles the daemon drives (its loops, its test
/// seams), and the routes they serve with the SPA behind them.
pub struct Contexts {
    pub platform: Arc<Platform>,
    pub session: Arc<Session>,
    pub run: Arc<pdo_run::Runs>,
    /// The daemon starts its heartbeat.
    pub triggers: Arc<pdo_trigger::Triggers>,
    /// The daemon drives its boot refresh.
    pub stats: Arc<pdo_stats::Stats>,
    /// The daemon warms its harness catalogues at boot.
    pub settings: Arc<pdo_settings::Settings>,
    /// The daemon applies its setting at boot.
    pub telemetry: Arc<pdo_telemetry::Telemetry>,
    /// The daemon feeds it the bus.
    pub telemetry_runs: pdo_telemetry::application::runs::RunRecorder,
    pub router: Router,
}

/// Wire every context. Called once per daemon, at boot.
pub(crate) fn wire(r: ContextResources, host: Arc<dyn ExtensionHost>) -> Contexts {
    let tmux = Arc::new(TmuxCli::for_daemon(r.port));
    wire_over(r, tmux, host)
}

/// [`wire`], with the session context over `tmux`: the daemon's private socket
/// in production, a fake in the composition root's own tests.
pub(crate) fn wire_over(
    r: ContextResources,
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
) -> Contexts {
    // The stored instance settings and the agentic profiles come first: the
    // upstream contexts read them through their own ports.
    let stored = StoredSettings {
        config: Arc::new(SqliteInstanceConfig::new(r.db.clone())),
        profiles: Arc::new(SqliteAgentProfiles::new(r.db.clone())),
    };
    let late_runs = Late::new();
    let late_platform = Late::new();
    let late_pipelines = Late::new();
    let workspace = workspace::workspace(&r, &late_pipelines, host.clone());
    let skills = skill::skills(&r.db, &r.repo_root, host.clone());
    let sandbox = sandbox::router(&r, host.clone());
    let session = session::session(&r, tmux, &late_runs, &late_platform, host.clone());
    let run = run::run(run::RunWiring {
        r: &r,
        session: &session,
        workspace: &workspace,
        skills: &skills,
        stored: &stored,
        platform: &late_platform,
        pipelines: &late_pipelines,
        host: host.clone(),
    });
    let review = review::review(&run, &stored, host.clone());
    let platform = platform::platform(&r, &run, &stored, host.clone());
    let stats = stats::stats(&r, &platform, &workspace, host.clone());
    let pipelines = pipeline::pipelines(&r, &run, &stats, &skills, host.clone());
    let triggers = trigger::triggers(trigger::TriggerWiring {
        db: r.db.clone(),
        repo_root: r.repo_root.clone(),
        panic_on_trigger_name: r.panic_on_trigger_name.clone(),
        settings: stored.config.clone(),
        run: &run,
        platform: &platform,
        host: host.clone(),
    });
    let (telemetry, telemetry_runs) =
        telemetry::telemetry(&r, &run, &session, &stored, host.clone());
    let settings = settings::settings(&r, stored, &platform, &telemetry, host.clone());
    late_runs.set(&run);
    late_platform.set(&platform);
    late_pipelines.set(&pipelines);

    // ROUTER TRAP: any NEW top-level path prefix (`/nodes`, `/stats`, `/fs`,
    // `/audit`, `/projects`, …) also needs its own entry in the Vite dev proxy
    // (`frontend/vite.config.ts`). Without it a dev-mode request answers 200 with
    // the SPA shell and any smoke test lies. Sub-paths of an already-proxied
    // prefix are free (the proxy key is a prefix), and a static segment beside a
    // `{param}` in the same subtree is fine — axum 0.8 prefers the static one.
    let router = Router::new()
        .fallback(static_handler)
        .merge(pdo_workspace::adapters::http::router::<ProjectTiers>(
            workspace,
        ))
        .merge(pdo_skill::adapters::http::router(skills))
        .merge(pdo_run::router(run.clone()))
        .merge(sandbox)
        .merge(pdo_platform::router(platform.clone()))
        .merge(pdo_session::router(session.clone()))
        .merge(pdo_pipeline::adapters::http::router(
            pipelines,
            ws_origin(r.port, r.allowed_ws_origins.clone()),
        ))
        .merge(pdo_review::router(review))
        .merge(pdo_stats::router(stats.clone()))
        .merge(pdo_trigger::router(triggers.clone()))
        .merge(pdo_settings::router(settings.clone()))
        .merge(pdo_telemetry::router(telemetry.clone()))
        .merge(crate::authorize::router(host, run.clone()));
    // CORE-5: every read of one Run, whichever context serves it, asks first.
    let router = pdo_run::adapters::http::read_guard::guard_run_reads(router, run.clone());
    // ADR-0090: every request measured while the export runs, by its route.
    let router = router.layer(axum::middleware::from_fn_with_state(
        telemetry.clone(),
        pdo_telemetry::adapters::http::measure,
    ));
    Contexts {
        router,
        platform,
        session,
        run,
        stats,
        triggers,
        settings,
        telemetry,
        telemetry_runs,
    }
}

/// The Origin guard of the daemon's WebSocket upgrades, for a context that
/// does not read the session context.
fn ws_origin(port: u16, extra: Vec<String>) -> pdo_pipeline::adapters::http::draft::OriginCheck {
    Arc::new(move |headers| pdo_session::adapters::http::origin::check_origin(headers, port, &extra))
}

/// The settings context's stores, which the upstream contexts read too.
struct StoredSettings {
    config: Arc<dyn InstanceConfigStore>,
    profiles: Arc<dyn AgentProfileStore>,
}

/// The contexts of a unit test: wired as the daemon wires them, over an
/// in-memory database and a temporary repository, with no extension, a node
/// session that runs `exec true`, and the tmux (and the service health) a test
/// hands in.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use pdo_platform::domain::service_health::ServiceHealthOverride;
    use pdo_run::domain::event::Event;
    use pdo_session::application::ports::Tmux;

    use super::{wire, wire_over, ContextResources, Contexts};

    /// What a test changes of the default wiring.
    #[derive(Default)]
    pub(crate) struct TestOptions {
        pub(crate) tmux: Option<Arc<dyn Tmux>>,
        pub(crate) service_health: Option<ServiceHealthOverride>,
    }

    pub(crate) struct TestContexts {
        pub(crate) db: sqlx::SqlitePool,
        pub(crate) repo_root: PathBuf,
        pub(crate) contexts: Contexts,
        _root: tempfile::TempDir,
    }

    impl TestContexts {
        pub(crate) async fn new() -> Self {
            Self::with(TestOptions::default()).await
        }

        pub(crate) async fn with(options: TestOptions) -> Self {
            let root = tempfile::tempdir().unwrap();
            let repo_root = root.path().to_path_buf();
            let db = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            crate::daemon::storage::init_db(&db, &repo_root)
                .await
                .unwrap();
            let (bus_tx, _) = tokio::sync::broadcast::channel(80);
            let resources = ContextResources {
                db: db.clone(),
                repo_root: repo_root.clone(),
                port: next_test_daemon_port(),
                home_root: None,
                update_source_url: None,
                release_channel: None,
                update_executor_override: None,
                install_method_override: None,
                supervision_override: None,
                relaunch_command: None,
                service_health_override: options.service_health,
                // Self-destructing no-op: should a test reach the spawn path, the
                // node session runs `true` and exits immediately — never real claude.
                tmux_cmd_override: Some("exec true".to_string()),
                allowed_ws_origins: Vec::new(),
                bus: Arc::new(pdo_kernel::event_bus::BroadcastEventBus::from_sender(
                    bus_tx.clone(),
                )),
                bus_tx,
                run_dirs: Arc::new(pdo_run::adapters::run_dir_watch::DebouncedRunDirs(
                    Arc::new(Mutex::new(None)),
                )),
                self_writes: pdo_pipeline::adapters::self_writes::SelfWrites::new(),
                docker_cmd_override: None,
                sandbox_home_override: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                panic_on_trigger_name: None,
                price_source_url: None,
                price_refresh_at_boot: false,
                project_names: Default::default(),
            };
            let host = Arc::new(pdo_kernel::extension_host::NoExtensions);
            let contexts = match options.tmux {
                Some(tmux) => wire_over(resources, tmux, host),
                None => wire(resources, host),
            };
            contexts.platform.init().await.unwrap();
            Self {
                db,
                repo_root,
                contexts,
                _root: root,
            }
        }

        pub(crate) fn repo_root(&self) -> &Path {
            &self.repo_root
        }

        /// Append `event` to its Run's log, as every event enters it.
        pub(crate) async fn append(&self, event: &Event) {
            pdo_run::application::events::append_event(&self.contexts.run, event)
                .await
                .unwrap();
        }

        pub(crate) async fn events(&self, run_id: &str) -> Vec<Event> {
            pdo_run::adapters::sqlite_event_store::load(&self.db, run_id)
                .await
                .unwrap()
        }
    }

    /// A unique fake daemon port per test, so the derived tmux socket
    /// (`pdo-<port>`) is distinct per test. Tests spawn stub sessions that exit
    /// immediately (`exec true`); on a *shared* socket one test's session exit
    /// tears the tmux server down just as a concurrent test's `new-session`
    /// runs, failing that spawn. This is a socket-name seed only, never a bound
    /// port.
    ///
    /// The counter is per PROCESS, and `make test` runs nextest — one process per
    /// test — so its start is spread by process id: 4 000 slots of 10 sockets
    /// across 20 000..60 000. `cargo test` (one process) still counts up from a
    /// single start, so it never collides.
    fn next_test_daemon_port() -> u16 {
        use std::sync::atomic::{AtomicU16, Ordering};
        static NEXT: std::sync::OnceLock<AtomicU16> = std::sync::OnceLock::new();
        NEXT.get_or_init(|| AtomicU16::new(20000 + (std::process::id() % 4000) as u16 * 10))
            .fetch_add(1, Ordering::Relaxed)
    }
}
