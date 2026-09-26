//! The wiring of the bounded contexts (ADR-0079): each context's adapters
//! plugged onto its ports, over the resources the legacy daemon opened.

use std::sync::Arc;

use pdo_daemon::{ContextResources, Contexts};
use pdo_kernel::extension_host::ExtensionHost;
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

/// Wire every extracted context. Called once per daemon, at boot.
pub(crate) fn wire(resources: ContextResources, host: Arc<dyn ExtensionHost>) -> Contexts {
    let platform = platform(resources, host);
    Contexts {
        router: pdo_platform::router(platform.clone()),
        platform,
    }
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
