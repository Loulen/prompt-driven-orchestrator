//! The `trigger` context: the Triggers over SQLite, the guard run by `sh`, the
//! Runs of the run context (and the Projets of a repository, matched as theirs), the library's pipelines on disk, the repositories
//! through git, the staging profiles over SQLite, the platform's audit journal,
//! the kernel's bus, and the pause and the guard timeout of the settings context.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::Platform;
use pdo_run::Runs;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_settings::adapters::contexts::SettingsOfTriggers;
use pdo_settings::application::ports::InstanceConfigStore;
use pdo_trigger::adapters::contexts::{
    GitRepositories, LibraryPipelines, PlatformAudit as TriggerAudit, ProjectsOfRuns,
    RunsOfTriggers, StagingProfiles,
};
use pdo_trigger::adapters::guard_shell::ShellGuard;
use pdo_trigger::adapters::sqlite::SqliteTriggers;
use pdo_trigger::{Triggers, TriggersOptions, TriggersPorts};

/// What the trigger context is wired over.
pub(super) struct TriggerWiring<'a> {
    pub(super) db: sqlx::SqlitePool,
    pub(super) repo_root: std::path::PathBuf,
    pub(super) panic_on_trigger_name: Option<String>,
    pub(super) settings: Arc<dyn InstanceConfigStore>,
    pub(super) run: &'a Arc<Runs>,
    pub(super) platform: &'a Arc<Platform>,
    pub(super) host: Arc<dyn ExtensionHost>,
}

pub(super) fn triggers(w: TriggerWiring<'_>) -> Arc<Triggers> {
    Triggers::new(
        TriggersPorts {
            store: Arc::new(SqliteTriggers::new(w.db.clone())),
            guards: Arc::new(ShellGuard),
            settings: Arc::new(SettingsOfTriggers(w.settings)),
            pipelines: Arc::new(LibraryPipelines {
                repo_root: w.repo_root.clone(),
            }),
            repositories: Arc::new(GitRepositories),
            projects: Arc::new(ProjectsOfRuns(w.run.clone())),
            sandbox_profiles: Arc::new(StagingProfiles(Arc::new(SqliteProfiles::new(w.db)))),
            runs: Arc::new(RunsOfTriggers(w.run.clone())),
            audit: Arc::new(TriggerAudit(w.platform.clone())),
            bus: w.run.bus().clone(),
            extensions: w.host,
        },
        TriggersOptions {
            repo_root: w.repo_root,
            panic_on_trigger_name: w.panic_on_trigger_name,
        },
    )
}
