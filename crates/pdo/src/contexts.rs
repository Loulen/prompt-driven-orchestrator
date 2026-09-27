//! The wiring of the bounded contexts (ADR-0079): each context's adapters
//! plugged onto its ports, over the resources the legacy daemon opened.

use std::sync::Arc;

use pdo_daemon::{ContextResources, Contexts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::adapters::events::BroadcastPipelineEvents;
use pdo_pipeline::adapters::library_fs::FsNodeLibrary;
use pdo_pipeline::adapters::registry::FsPipelineRegistry;
use pdo_pipeline::adapters::skill_bank::BankOfSkills;
use pdo_pipeline::{Pipelines, PipelinesPorts};
use pdo_platform::adapters::detached_executor::DetachedExecutor;
use pdo_platform::adapters::fs_update_journal::FsUpdateJournal;
use pdo_platform::adapters::github_releases::GithubReleases;
use pdo_platform::adapters::host_installation::HostInstallation;
use pdo_platform::adapters::page_files::LocalPageFiles;
use pdo_platform::adapters::service_health_probe;
use pdo_platform::adapters::sqlite_audit_log::SqliteAuditLog;
use pdo_platform::adapters::sqlite_page_mounts::SqlitePageMounts;
use pdo_platform::application::update_context::UpdateOptions;
use pdo_platform::{Platform, PlatformPorts};
use pdo_run::adapters::manager_sessions::TmuxManagerSessions;
use pdo_run::adapters::sqlite_event_store::SqliteEventStore;
use pdo_run::{Runs, RunsOptions, RunsPorts};
use pdo_session::adapters::terminal_window::OsTerminalWindow;
use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::{Session, SessionOptions, SessionPorts};

/// Wire every extracted context. Called once per daemon, at boot.
pub(crate) fn wire(resources: ContextResources, host: Arc<dyn ExtensionHost>) -> Contexts {
    let session = session(&resources, host.clone());
    let run = run(&resources, &session);
    let pipelines = pipelines(&resources, host.clone());
    let platform = platform(resources, host);
    Contexts {
        router: pdo_platform::router(platform.clone())
            .merge(pdo_session::router(session.clone()))
            .merge(pdo_pipeline::adapters::http::router(pipelines)),
        platform,
        session,
        run,
    }
}

/// The `run` context: the event log over SQLite, the kernel's bus the daemon
/// opened, the manager probe over the session's tmux, and what the legacy
/// daemon still reads from disk for a Run's detail.
fn run(r: &ContextResources, session: &Session) -> Arc<Runs> {
    Runs::new(
        RunsPorts {
            store: Arc::new(SqliteEventStore::new(r.db.clone())),
            bus: r.run.bus.clone(),
            live_facts: r.run.live_facts.clone(),
            managers: Arc::new(TmuxManagerSessions(session.tmux().clone())),
        },
        RunsOptions {
            daemon_port: r.port,
            allowed_ws_origins: r.allowed_ws_origins.clone(),
        },
    )
}

/// The `session` context: tmux on the daemon's private socket, the OS terminal
/// window, and the Run-side ports the legacy daemon still implements.
fn session(r: &ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Session> {
    let run = r.session.clone();
    Session::new(
        SessionPorts {
            tmux: Arc::new(TmuxCli::for_daemon(r.port)),
            waits: run.waits,
            panes: run.panes,
            shells: run.shells,
            owners: run.owners,
            gauges: run.gauges,
            templates: run.templates,
            assistant_harness: run.assistant_harness,
            terminal_window: Arc::new(OsTerminalWindow),
            extensions: host,
        },
        SessionOptions {
            daemon_port: r.port,
            repo_root: r.repo_root.clone(),
            tmux_cmd_override: r.tmux_cmd_override.clone(),
            allowed_ws_origins: r.allowed_ws_origins.clone(),
        },
    )
}

/// The `pipeline` context: the instance registry and the node library on
/// disk, the daemon's live stream, the skill bank, and the Run-side and
/// Stats-side ports the legacy daemon still implements.
fn pipelines(r: &ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Pipelines> {
    let p = r.pipeline.clone();
    Arc::new(Pipelines::new(PipelinesPorts {
        registry: Arc::new(FsPipelineRegistry::new(r.repo_root.clone(), p.self_writes)),
        library: Arc::new(FsNodeLibrary::home()),
        events: Arc::new(BroadcastPipelineEvents(p.events)),
        runs: p.runs,
        renames: p.renames,
        skills: Arc::new(BankOfSkills(p.skills)),
        extensions: host,
    }))
}

/// The `platform` context: SQLite for the mounts and the audit, the local
/// filesystem for the pages and the update journal, GitHub for the releases,
/// a detached `sh` for the update.
fn platform(r: ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Platform> {
    let ports = PlatformPorts {
        page_mounts: Arc::new(SqlitePageMounts::new(r.db.clone())),
        page_files: Arc::new(LocalPageFiles),
        audit: Arc::new(SqliteAuditLog::new(r.db)),
        runs: r.runs,
        update_setting: r.update_setting,
        release_source: Arc::new(GithubReleases),
        update_journal: Arc::new(FsUpdateJournal::new(r.home_root)),
        installation: Arc::new(HostInstallation::new(
            r.install_method_override,
            r.supervision_override,
        )),
        update_executor: Arc::new(DetachedExecutor::new(r.update_executor_override)),
        extensions: host,
    };
    let update = UpdateOptions {
        source_url: r.update_source_url,
        port: r.port,
        working_dir: r.repo_root,
        relaunch_command: r.relaunch_command,
    };
    // Observed once at boot, so the polled `GET /sessions` never shells out.
    let health = service_health_probe::observe(r.service_health_override);
    Platform::new(ports, update, health)
}
