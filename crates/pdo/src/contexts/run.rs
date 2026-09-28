//! The `run` context: the event log over SQLite, the kernel's bus, what a
//! Run's detail reads from disk, tmux and the declared wait of the session
//! context, the Projets of the workspace, the skills and their bank, the
//! staging profiles over SQLite, the pipeline watcher and the daemon's own
//! writes, the instance settings and the agentic profiles of the settings
//! context, and the page mounts an archive releases (the platform's).

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::adapters::skill_bank::BankOfSkills;
use pdo_platform::Platform;
use pdo_run::adapters::contexts::{ContextSkills, SessionWaits, WorkspaceProjects};
use pdo_run::adapters::disk_live_facts::DiskLiveFacts;
use pdo_run::adapters::manager_sessions::TmuxManagerSessions;
use pdo_run::adapters::sqlite_event_store::SqliteEventStore;
use pdo_run::{Runs, RunsOptions, RunsPorts};
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_session::Session;
use pdo_settings::adapters::contexts::RunInstanceSettings;
use pdo_skill::Skills;
use pdo_workspace::application::Workspace;

use super::{ContextResources, StoredSettings};
use crate::ports::run_pages::PlatformPages;
use crate::ports::Late;

/// What the run context is wired over.
pub(super) struct RunWiring<'a> {
    pub r: &'a ContextResources,
    pub session: &'a Arc<Session>,
    pub workspace: &'a Arc<Workspace>,
    pub skills: &'a Arc<Skills>,
    pub stored: &'a StoredSettings,
    pub platform: &'a Late<Platform>,
    pub host: Arc<dyn ExtensionHost>,
}

pub(super) fn run(w: RunWiring<'_>) -> Arc<Runs> {
    let r = w.r;
    Runs::new(
        RunsPorts {
            store: Arc::new(SqliteEventStore::new(r.db.clone())),
            bus: r.bus.clone(),
            live_facts: Arc::new(DiskLiveFacts::new(
                r.repo_root.clone(),
                r.sandbox_home_override.clone(),
            )),
            managers: Arc::new(TmuxManagerSessions(w.session.tmux().clone())),
            tmux: w.session.tmux().clone(),
            waits: Arc::new(SessionWaits(w.session.clone())),
            projects: Arc::new(WorkspaceProjects(w.workspace.clone())),
            skills: Arc::new(ContextSkills(w.skills.clone())),
            sandbox_profiles: Arc::new(SqliteProfiles::new(r.db.clone())),
            settings: Arc::new(RunInstanceSettings {
                config: w.stored.config.clone(),
                profiles: w.stored.profiles.clone(),
            }),
            bank: Arc::new(BankOfSkills(w.skills.clone())),
            pages: Arc::new(PlatformPages(w.platform.clone())),
            run_dirs: r.run_dirs.clone(),
            self_writes: r.self_writes.clone(),
            extensions: w.host,
        },
        RunsOptions {
            daemon_port: r.port,
            allowed_ws_origins: r.allowed_ws_origins.clone(),
            repo_root: r.repo_root.clone(),
            tmux_cmd_override: r.tmux_cmd_override.clone(),
            docker_cmd_override: r.docker_cmd_override.clone(),
            sandbox_home_override: r.sandbox_home_override.clone(),
            panic_on_stale_sweep: r.panic_on_stale_sweep,
            panic_on_spawn: r.panic_on_spawn,
        },
    )
}
