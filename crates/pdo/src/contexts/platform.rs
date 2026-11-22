//! The `platform` context: SQLite for the mounts and the audit, the local
//! filesystem for the pages and the update journal, GitHub for the releases,
//! a detached `sh` for the update, the Runs of the run context, and its
//! `update_check` and `public_url` instance settings.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::adapters::detached_executor::DetachedExecutor;
use pdo_platform::adapters::fs_update_journal::FsUpdateJournal;
use pdo_platform::adapters::github_releases::GithubReleases;
use pdo_platform::adapters::host_installation::HostInstallation;
use pdo_platform::adapters::page_files::LocalPageFiles;
use pdo_platform::adapters::service_health_probe;
use pdo_platform::adapters::sqlite_audit_log::SqliteAuditLog;
use pdo_platform::adapters::sqlite_bug_report_choices::SqliteBugReportChoices;
use pdo_platform::adapters::sqlite_page_mounts::SqlitePageMounts;
use pdo_platform::adapters::sqlite_update_snapshot::SqliteUpdateSnapshot;
use pdo_platform::application::update_context::UpdateOptions;
use pdo_platform::domain::release_channel::{UpdateChannel, Upgrade};
use pdo_platform::{Platform, PlatformPorts};
use pdo_run::Runs;
use pdo_settings::adapters::contexts::{PublicUrlOfPlatform, UpdateCheckOfPlatform};

use super::{ContextResources, StoredSettings};
use crate::ports::platform_runs::RunsOfPlatform;

pub(super) fn platform(
    r: &ContextResources,
    run: &Arc<Runs>,
    stored: &StoredSettings,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Platform> {
    let ports = PlatformPorts {
        page_mounts: Arc::new(SqlitePageMounts::new(r.db.clone())),
        page_files: Arc::new(LocalPageFiles),
        audit: Arc::new(SqliteAuditLog::new(r.db.clone())),
        runs: Arc::new(RunsOfPlatform(run.clone())),
        update_setting: Arc::new(UpdateCheckOfPlatform(stored.config.clone())),
        public_url: Arc::new(PublicUrlOfPlatform {
            config: stored.config.clone(),
            default: r.public_url_default.clone(),
        }),
        release_source: Arc::new(GithubReleases),
        update_journal: Arc::new(FsUpdateJournal::new(r.home_root.clone())),
        // A daemon whose home is overridden (the test seam) looks for the
        // cargo-dist receipt under that home, never under the real one.
        installation: Arc::new(
            HostInstallation::new(r.install_method_override, r.supervision_override)
                .with_config_home(r.sandbox_home_override.as_ref().map(|h| h.join(".config"))),
        ),
        update_executor: Arc::new(DetachedExecutor::new(r.update_executor_override.clone())),
        // The core's database and every database under the extensions' data
        // folder: what the Retour automatique puts back.
        update_snapshot: Arc::new(SqliteUpdateSnapshot::new(
            vec![r.repo_root.join(".pdo").join("pdo.db")],
            vec![crate::serve::extensions_data_root_of(
                r.home_root.as_deref(),
                &r.repo_root,
            )],
        )),
        bug_report_choices: Arc::new(SqliteBugReportChoices::new(r.db.clone())),
        extensions: host,
    };
    let mut channel = r
        .release_channel
        .clone()
        .unwrap_or_else(UpdateChannel::vanilla);
    // The test seams of an Update (unset in production).
    let seams = &r.update_seams;
    if let Some(command) = &seams.upgrade_command {
        channel.upgrade = Upgrade::Declared(Some(command.clone()));
    }
    if let Some(version) = &seams.installed_version {
        channel.installed_version = version.clone();
    }
    let update = UpdateOptions {
        // The vanilla binary's defaults are explicit: `UpdateChannel::vanilla()`.
        channel,
        // `PDO_UPDATE_SOURCE_URL` wins over the channel's source (test seam).
        source_url: r.update_source_url.clone(),
        port: r.port,
        working_dir: r.repo_root.clone(),
        relaunch_command: r.relaunch_command.clone(),
        verify_timeout_secs: seams.verify_timeout_secs,
    };
    // Observed once at boot, so the polled `GET /sessions` never shells out.
    let health = service_health_probe::observe(r.service_health_override);
    Platform::new(ports, update, health)
}
