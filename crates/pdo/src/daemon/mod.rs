//! The daemon: open the database, wire the contexts, bind the listener, start
//! the background loops, serve.

mod background;
pub(crate) mod config;
pub(crate) mod env;
mod handle;
mod origins;
pub(crate) mod router_layer;
pub(crate) mod storage;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use pdo_kernel::project_names::ProjectNames;
use pdo_pipeline::adapters::self_writes::SelfWrites;
use pdo_pipeline::adapters::watcher as pipeline_watcher;
use pdo_run::application::boot_recovery;
use pdo_run::application::paths::instance_pipeline_dir;
use pdo_workspace::adapters::sqlite::SqliteProjects;
use tokio::sync::broadcast;
use tracing::{info, warn};

pub use config::{DaemonConfig, ExtensionHostHandle};
pub use handle::DaemonHandle;

use crate::contexts::{ContextResources, Contexts};
use crate::ports::project_events::load_project_names;
use background::Loops;
use router_layer::RouterLayer;

/// Start a daemon with an explicit [`DaemonConfig`]. Its
/// [`DaemonConfig::router_layer`] must carry the wiring of the contexts
/// (`pdo::serve`, `pdo::composed_router_layer`). Tests use this to seed a
/// per-daemon `tmux_cmd_override` without mutating process-global env.
pub async fn serve_with_config(
    addr: SocketAddr,
    repo_root: PathBuf,
    config: DaemonConfig,
) -> Result<DaemonHandle> {
    let (db, db_path) = storage::open(&repo_root).await?;
    storage::prepare(&db, &db_path, &repo_root, config.extension_host.0.as_ref()).await?;

    // The kernel's event bus (D20): every Run event and every announcement `/ws`
    // relays, as `/ws` relays it. One channel, sized for the two it replaced
    // (256 Run events + 64 announcements).
    let (bus_tx, _) = broadcast::channel::<serde_json::Value>(320);
    let bus: Arc<dyn pdo_kernel::event_bus::EventBus> = Arc::new(
        pdo_kernel::event_bus::BroadcastEventBus::from_sender(bus_tx.clone()),
    );

    // The telemetry reads the Runs' events from the first one on.
    let telemetry_events = bus_tx.subscribe();

    let self_writes = SelfWrites::new();

    let (run_modified_tx, run_modified_rx) =
        tokio::sync::mpsc::unbounded_channel::<pipeline_watcher::RunPipelineModified>();

    let watcher = pipeline_watcher::spawn_watcher(
        repo_root.clone(),
        instance_pipeline_dir(&repo_root),
        bus_tx.clone(),
        self_writes.clone(),
        run_modified_tx,
    );
    let run_watcher = Arc::new(Mutex::new(watcher));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind")?;
    let bound_addr = listener
        .local_addr()
        .context("failed to read bound local addr")?;

    let wire_contexts = config
        .router_layer
        .as_ref()
        .and_then(RouterLayer::contexts)
        .context(
            "the daemon runs under the composition root (`pdo::serve`), which wires its \
             bounded contexts: no context wiring was given",
        )?;
    let project_names = Arc::new(ProjectNames::default());
    let resources = ContextResources {
        db: db.clone(),
        repo_root: repo_root.clone(),
        port: bound_addr.port(),
        home_root: config
            .sandbox_home_override
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from)),
        update_source_url: config.update_source_url.clone(),
        release_channel: config.release_channel.clone(),
        brand: config.brand,
        update_executor_override: config.update_executor_override.clone(),
        install_method_override: config.install_method_override,
        supervision_override: config.supervision_override,
        relaunch_command: config.relaunch_command.clone(),
        service_health_override: config.service_health_override,
        tmux_cmd_override: config.tmux_cmd_override.clone(),
        allowed_ws_origins: config.allowed_ws_origins.clone(),
        bus,
        bus_tx,
        run_dirs: Arc::new(pdo_run::adapters::run_dir_watch::DebouncedRunDirs(
            run_watcher.clone(),
        )),
        self_writes,
        docker_cmd_override: config.docker_cmd_override.clone(),
        sandbox_home_override: config.sandbox_home_override.clone(),
        panic_on_stale_sweep: config.panic_on_stale_sweep,
        panic_on_spawn: config.panic_on_spawn,
        panic_on_trigger_name: config.panic_on_trigger_name.clone(),
        price_source_url: config.price_source_url.clone(),
        price_refresh_at_boot: config.price_refresh_at_boot,
        project_names: project_names.clone(),
    };
    let Contexts {
        platform,
        session,
        run,
        triggers,
        stats,
        settings,
        telemetry,
        telemetry_runs,
        router,
    } = wire_contexts(resources);
    // ADR-0090: the stored setting applies from the start; off, nothing is
    // built and nothing connects.
    telemetry.reload().await;
    // API 0.11: the extensions read the names of the Projets from the first
    // request on.
    load_project_names(&SqliteProjects::new(db.clone()), &project_names).await;
    platform
        .init()
        .await
        .map_err(|e| anyhow::anyhow!("failed to create the platform tables: {e}"))?;

    // A nested daemon suppresses every AUTOMATIC cleanup pathway, so a Tester or
    // Implementer that accidentally runs `pdo daemon` can't trigger reaper-based
    // kills. It still serves HTTP and accepts explicit `cleanup_run` calls.
    if config.nested_daemon {
        warn!(
            "PDO daemon in nested (no-cleanup) mode — \
             skipping boot-time orphan sweep and periodic reaper. \
             This daemon will not auto-reap any tmux sessions."
        );
    } else {
        if let Err(e) = session.sweep().run().await {
            warn!("Orphan sweep at boot failed: {e}");
        }

        // Reconcile persisted run state against the live process world: fail-fast
        // on orphaned Running nodes and surface git/event-log divergence.
        boot_recovery::run_boot_recovery(&run).await;
    }
    // ADR-0092: the Sessions hébergées a previous daemon held are taken back
    // with who started them, or ended if their process is gone. Before the
    // ledger adopts the live sessions, so it knows who started them.
    let hosted = session.hosted().clone();
    if let Err(e) = tokio::task::spawn_blocking(move || hosted.restore()).await {
        warn!("the hosted sessions could not be restored: {e}");
    }
    // CORE-61: the sessions a previous daemon started end like the others, and
    // their end is announced to the extensions.
    let ledger = session.ledger().clone();
    if let Err(e) = tokio::task::spawn_blocking(move || ledger.adopt_live()).await {
        warn!("the live sessions could not be adopted: {e}");
    }

    let app = match &config.router_layer {
        Some(layer) => layer.apply(router),
        None => router,
    };

    // #541: the bind address (`0.0.0.0`) is deliberate (#260) but is NOT a URL a
    // browser can use — the WS Origin guard (#564) only knows `localhost` and
    // `127.0.0.1`, so `http://0.0.0.0:<port>` half-loads the UI and 403s every
    // socket. Advertise the URL that actually works, keep the bind for operators.
    info!(
        "PDO daemon listening on {bound_addr} — open {}",
        origins::advertised_url(bound_addr)
    );
    origins::log_allowed_ws_origins(&config.allowed_ws_origins);

    let loops = Loops::start(&run, &session, &triggers, &config)
        .relay_pipeline_modifications(&run, run_modified_rx)
        .record_telemetry(&telemetry, telemetry_runs, telemetry_events);

    let task = tokio::spawn(async move {
        let _watcher = run_watcher; // keep the file watcher alive for the server's lifetime
        let _loops = loops; // keep the background loops alive
        axum::serve(listener, app).await.context("server error")?;
        Ok(())
    });

    // Must stay DETACHED and posted AFTER `tokio::spawn(axum::serve(...))`: see
    // `background::start_detached`.
    background::start_detached(&stats, &platform, &settings, config.run_update_check_loop);

    Ok(DaemonHandle {
        addr: bound_addr,
        task,
        run,
        session,
        stats,
        platform,
        triggers,
        settings,
    })
}
