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
use pdo_review::adapters::git::CliGit;
use pdo_review::adapters::runs::ContextRuns;
use pdo_review::{Review, ReviewPorts};
use pdo_run::adapters::contexts::{ContextSkills, SessionWaits, WorkspaceProjects};
use pdo_run::adapters::disk_live_facts::DiskLiveFacts;
use pdo_run::adapters::manager_sessions::TmuxManagerSessions;
use pdo_run::adapters::sqlite_event_store::SqliteEventStore;
use pdo_run::{Runs, RunsOptions, RunsPorts};
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_session::adapters::terminal_window::OsTerminalWindow;
use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::{Session, SessionOptions, SessionPorts};
use pdo_stats::adapters::contexts::{
    BusAnnouncements, PlatformAudit, WorkspaceProjects as StatsProjects,
};
use pdo_stats::adapters::models_dev::HttpPriceSource;
use pdo_stats::adapters::price_files::HomePriceFiles;
use pdo_stats::adapters::rename_ledger::StatsRenames;
use pdo_stats::adapters::run_events::RunContextEvents;
use pdo_stats::adapters::run_files::DiskRunFiles;
use pdo_stats::adapters::sqlite_absorptions::SqliteAbsorptions;
use pdo_stats::adapters::transcripts::HarnessTranscripts;
use pdo_stats::{Stats, StatsOptions, StatsPorts};
use pdo_trigger::adapters::contexts::{
    GitRepositories, LibraryPipelines, PlatformAudit as TriggerAudit, RunsOfTriggers,
    StagingProfiles,
};
use pdo_trigger::adapters::guard_shell::ShellGuard;
use pdo_trigger::adapters::sqlite::SqliteTriggers;
use pdo_trigger::{Triggers, TriggersOptions, TriggersPorts};

/// Wire every extracted context. Called once per daemon, at boot.
pub(crate) fn wire(resources: ContextResources, host: Arc<dyn ExtensionHost>) -> Contexts {
    let session = session(&resources, host.clone());
    let run = run(&resources, &session, host.clone());
    let review = review(&resources, &run, host.clone());
    let platform = platform(&resources, host.clone());
    let stats = stats(&resources, &platform, host.clone());
    let pipelines = pipelines(&resources, &stats, host.clone());
    let triggers = triggers(TriggerWiring {
        db: resources.db.clone(),
        repo_root: resources.repo_root.clone(),
        legacy: resources.trigger.clone(),
        run: &run,
        platform: &platform,
        host,
    });
    Contexts {
        router: pdo_platform::router(platform.clone())
            .merge(pdo_session::router(session.clone()))
            .merge(pdo_pipeline::adapters::http::router(pipelines))
            .merge(pdo_review::router(review))
            .merge(pdo_stats::router(stats.clone()))
            .merge(pdo_trigger::router(triggers.clone())),
        platform,
        session,
        run,
        stats,
        triggers,
    }
}

/// The `run` context: the event log over SQLite, the kernel's bus the daemon
/// opened, what a Run's detail reads from disk, tmux and the declared wait of
/// the session context, the Projets of the workspace, the skills and their
/// bank, the staging profiles over SQLite, the pipeline watcher and the
/// daemon's own writes, and what the legacy daemon still implements (the
/// instance settings, the page mounts an archive releases).
fn run(r: &ContextResources, session: &Arc<Session>, host: Arc<dyn ExtensionHost>) -> Arc<Runs> {
    let legacy = r.run.clone();
    Runs::new(
        RunsPorts {
            store: Arc::new(SqliteEventStore::new(r.db.clone())),
            bus: legacy.bus,
            live_facts: Arc::new(DiskLiveFacts::new(
                r.repo_root.clone(),
                legacy.sandbox_home_override.clone(),
            )),
            managers: Arc::new(TmuxManagerSessions(session.tmux().clone())),
            tmux: session.tmux().clone(),
            waits: Arc::new(SessionWaits(session.clone())),
            projects: Arc::new(WorkspaceProjects(legacy.workspace)),
            skills: Arc::new(ContextSkills(legacy.skills.clone())),
            sandbox_profiles: Arc::new(SqliteProfiles::new(r.db.clone())),
            settings: legacy.settings,
            bank: Arc::new(BankOfSkills(legacy.skills)),
            pages: legacy.pages,
            run_dirs: legacy.run_dirs,
            self_writes: legacy.self_writes,
            extensions: host,
        },
        RunsOptions {
            daemon_port: r.port,
            allowed_ws_origins: r.allowed_ws_origins.clone(),
            repo_root: r.repo_root.clone(),
            tmux_cmd_override: r.tmux_cmd_override.clone(),
            docker_cmd_override: legacy.docker_cmd_override,
            sandbox_home_override: legacy.sandbox_home_override,
            panic_on_stale_sweep: legacy.panic_on_stale_sweep,
            panic_on_spawn: legacy.panic_on_spawn,
        },
    )
}

/// What the trigger context is wired over.
struct TriggerWiring<'a> {
    db: sqlx::SqlitePool,
    repo_root: std::path::PathBuf,
    legacy: pdo_daemon::TriggerResources,
    run: &'a Arc<Runs>,
    platform: &'a Arc<Platform>,
    host: Arc<dyn ExtensionHost>,
}

/// The `trigger` context: the Triggers over SQLite, the guard run by `sh`, the
/// Runs of the run context, the library's pipelines on disk, the repositories
/// through git, the staging profiles over SQLite, the platform's audit journal,
/// the kernel's bus, and the instance settings the legacy daemon still holds.
fn triggers(w: TriggerWiring<'_>) -> Arc<Triggers> {
    Triggers::new(
        TriggersPorts {
            store: Arc::new(SqliteTriggers::new(w.db.clone())),
            guards: Arc::new(ShellGuard),
            settings: w.legacy.settings,
            pipelines: Arc::new(LibraryPipelines {
                repo_root: w.repo_root.clone(),
            }),
            repositories: Arc::new(GitRepositories),
            sandbox_profiles: Arc::new(StagingProfiles(Arc::new(SqliteProfiles::new(w.db)))),
            runs: Arc::new(RunsOfTriggers(w.run.clone())),
            audit: Arc::new(TriggerAudit(w.platform.clone())),
            bus: w.run.bus().clone(),
            extensions: w.host,
        },
        TriggersOptions {
            repo_root: w.repo_root,
            panic_on_trigger_name: w.legacy.panic_on_trigger_name,
        },
    )
}

/// The `review` context: the Runs of the run context, git through the
/// workspace's adapter, and the instance setting the legacy daemon still owns.
fn review(r: &ContextResources, run: &Arc<Runs>, host: Arc<dyn ExtensionHost>) -> Arc<Review> {
    Review::new(ReviewPorts {
        runs: Arc::new(ContextRuns(run.clone())),
        git: Arc::new(CliGit),
        settings: r.review_settings.clone(),
        extensions: host,
    })
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

/// The `stats` context: the run context's event log (its `EventStore` and its
/// cross-Run reads over SQLite), the absorptions over SQLite, the Runs' files
/// and transcripts on disk, the price files of the host home and models.dev,
/// the Projets of the workspace, the platform's audit journal, the kernel's
/// bus, and the Triggers' fires the legacy daemon still holds.
fn stats(
    r: &ContextResources,
    platform: &Arc<Platform>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Stats> {
    let store = Arc::new(SqliteEventStore::new(r.db.clone()));
    let home_override = r.run.sandbox_home_override.clone();
    Stats::new(
        StatsPorts {
            events: Arc::new(RunContextEvents::new(store.clone(), store)),
            triggers: r.stats.triggers.clone(),
            projects: Arc::new(StatsProjects(r.run.workspace.clone())),
            absorptions: Arc::new(SqliteAbsorptions::new(r.db.clone())),
            run_files: Arc::new(DiskRunFiles::new(
                r.repo_root.clone(),
                home_override.clone(),
            )),
            transcripts: Arc::new(HarnessTranscripts),
            price_files: Arc::new(HomePriceFiles::new(home_override)),
            price_source: Arc::new(HttpPriceSource),
            audit: Arc::new(PlatformAudit(platform.clone())),
            announcements: Arc::new(BusAnnouncements(r.run.bus.clone())),
            extensions: host,
        },
        StatsOptions {
            repo_root: r.repo_root.clone(),
            price_source_url: r.stats.price_source_url.clone(),
            price_refresh_at_boot: r.stats.price_refresh_at_boot,
        },
    )
}

/// The `pipeline` context: the instance registry and the node library on
/// disk, the daemon's live stream, the skill bank, the Stats absorption a
/// rename leaves behind, and the Run-side port the legacy daemon still
/// implements.
fn pipelines(
    r: &ContextResources,
    stats: &Arc<Stats>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Pipelines> {
    let p = r.pipeline.clone();
    Arc::new(Pipelines::new(PipelinesPorts {
        registry: Arc::new(FsPipelineRegistry::new(r.repo_root.clone(), p.self_writes)),
        library: Arc::new(FsNodeLibrary::home()),
        events: Arc::new(BroadcastPipelineEvents(p.events)),
        runs: p.runs,
        renames: Arc::new(StatsRenames(stats.clone())),
        skills: Arc::new(BankOfSkills(p.skills)),
        extensions: host,
    }))
}

/// The `platform` context: SQLite for the mounts and the audit, the local
/// filesystem for the pages and the update journal, GitHub for the releases,
/// a detached `sh` for the update.
fn platform(r: &ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Platform> {
    let ports = PlatformPorts {
        page_mounts: Arc::new(SqlitePageMounts::new(r.db.clone())),
        page_files: Arc::new(LocalPageFiles),
        audit: Arc::new(SqliteAuditLog::new(r.db.clone())),
        runs: r.runs.clone(),
        update_setting: r.update_setting.clone(),
        release_source: Arc::new(GithubReleases),
        update_journal: Arc::new(FsUpdateJournal::new(r.home_root.clone())),
        installation: Arc::new(HostInstallation::new(
            r.install_method_override,
            r.supervision_override,
        )),
        update_executor: Arc::new(DetachedExecutor::new(r.update_executor_override.clone())),
        extensions: host,
    };
    let update = UpdateOptions {
        source_url: r.update_source_url.clone(),
        port: r.port,
        working_dir: r.repo_root.clone(),
        relaunch_command: r.relaunch_command.clone(),
    };
    // Observed once at boot, so the polled `GET /sessions` never shells out.
    let health = service_health_probe::observe(r.service_health_override);
    Platform::new(ports, update, health)
}
