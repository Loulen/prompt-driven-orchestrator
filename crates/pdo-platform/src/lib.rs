//! The `platform` bounded context (ADR-0079): the daemon as an installed piece
//! of software.
//!
//! - **Page mounts**: a directory served live under `/pages/<name>/`.
//! - **Update**: the version check, « What's new », the detached self-update.
//! - **Audit**: the journal of the configuration mutations made outside a Run.
//! - **Service**: the OS unit that keeps the daemon up (`pdo service`).
//!
//! Hexagonal inside: `domain/` holds the pure rules, `application/` the ports
//! and one use case per file, `adapters/` the HTTP routes, the CLI and the
//! implementations of the ports. The composition root builds a [`Platform`]
//! from the adapters and serves [`router`].

pub mod adapters;
pub mod application;
pub mod domain;

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;

use application::apply_update::ApplyUpdate;
use application::check_for_update::CheckForUpdate;
use application::get_changelog::GetChangelog;
use application::get_update_status::GetUpdateStatus;
use application::list_audit::ListAudit;
use application::mount_page::MountPage;
use application::page_journal::PageMountLock;
use application::place_page_mount::PlacePageMount;
use application::ports::{
    AuditLog, Installation, PageFiles, PageMountRepository, PublicUrlSetting, ReleaseSource, Runs,
    StorageError, UpdateCheckSetting, UpdateExecutor, UpdateJournal,
};
use application::read_attempt_log::ReadAttemptLog;
use application::read_pages::ReadPages;
use application::record_audit::RecordAudit;
use application::release_run_pages::ReleaseRunPages;
use application::unmount_page::UnmountPage;
use application::update_context::{UpdateContext, UpdateOptions, UpdatePorts};
use domain::service_health::ServiceHealth;

pub use adapters::http::router;

/// Every port the context needs, as the composition root plugs them in.
pub struct PlatformPorts {
    pub page_mounts: Arc<dyn PageMountRepository>,
    pub page_files: Arc<dyn PageFiles>,
    pub audit: Arc<dyn AuditLog>,
    /// The Runs (implemented by the `run` context).
    pub runs: Arc<dyn Runs>,
    /// The `update_check` setting (implemented by the `settings` context).
    pub update_setting: Arc<dyn UpdateCheckSetting>,
    /// The `public_url` setting (implemented by the `settings` context).
    pub public_url: Arc<dyn PublicUrlSetting>,
    pub release_source: Arc<dyn ReleaseSource>,
    pub update_journal: Arc<dyn UpdateJournal>,
    pub installation: Arc<dyn Installation>,
    pub update_executor: Arc<dyn UpdateExecutor>,
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The context, assembled: its use cases over its ports.
pub struct Platform {
    pub(crate) mount_page: MountPage,
    pub(crate) unmount_page: UnmountPage,
    pub(crate) read_pages: ReadPages,
    pub(crate) release_run_pages: ReleaseRunPages,
    pub(crate) record_audit: RecordAudit,
    pub(crate) list_audit: ListAudit,
    pub(crate) check_for_update: CheckForUpdate,
    pub(crate) get_update_status: GetUpdateStatus,
    pub(crate) get_changelog: GetChangelog,
    pub(crate) apply_update: ApplyUpdate,
    pub(crate) read_attempt_log: ReadAttemptLog,
    place_page_mount: Arc<PlacePageMount>,
    page_mounts: Arc<dyn PageMountRepository>,
    audit: Arc<dyn AuditLog>,
    public_url: Arc<dyn PublicUrlSetting>,
    service_health: ServiceHealth,
}

impl Platform {
    /// Assemble the context. `service_health` is observed once, at boot.
    pub fn new(
        ports: PlatformPorts,
        update: UpdateOptions,
        service_health: ServiceHealth,
    ) -> Arc<Self> {
        let PlatformPorts {
            page_mounts,
            page_files,
            audit,
            runs,
            update_setting,
            public_url,
            release_source,
            update_journal,
            installation,
            update_executor,
            extensions,
        } = ports;
        let lock = PageMountLock::default();
        let place_page_mount = Arc::new(PlacePageMount::new(page_mounts.clone(), runs.clone()));
        let record_audit = RecordAudit::new(audit.clone());
        let context = UpdateContext::new(
            UpdatePorts {
                setting: update_setting,
                source: release_source,
                journal: update_journal,
                installation,
                executor: update_executor,
                runs: runs.clone(),
            },
            update,
        );
        Arc::new(Self {
            mount_page: MountPage::new(
                page_mounts.clone(),
                page_files.clone(),
                runs.clone(),
                record_audit.clone(),
                extensions.clone(),
                lock.clone(),
                place_page_mount.clone(),
            ),
            unmount_page: UnmountPage::new(
                page_mounts.clone(),
                runs,
                record_audit.clone(),
                extensions.clone(),
                lock.clone(),
                place_page_mount.clone(),
            ),
            place_page_mount,
            read_pages: ReadPages::new(page_mounts.clone(), page_files),
            release_run_pages: ReleaseRunPages::new(page_mounts.clone(), lock),
            record_audit,
            list_audit: ListAudit::new(audit.clone(), extensions.clone()),
            check_for_update: CheckForUpdate::new(context.clone(), extensions.clone()),
            get_update_status: GetUpdateStatus::new(context.clone()),
            get_changelog: GetChangelog::new(context.clone()),
            apply_update: ApplyUpdate::new(context.clone(), extensions),
            read_attempt_log: ReadAttemptLog::new(context),
            page_mounts,
            audit,
            public_url,
            service_health,
        })
    }

    /// Create the context's storage if it does not exist yet.
    pub async fn init(&self) -> Result<(), StorageError> {
        self.audit.init().await?;
        self.page_mounts.init().await
    }

    /// Journal an out-of-Run configuration mutation, best-effort (ADR-0044).
    pub fn record_audit(&self) -> &RecordAudit {
        &self.record_audit
    }

    /// The page mounts of a Run being archived.
    pub fn release_run_pages(&self) -> &ReleaseRunPages {
        &self.release_run_pages
    }

    /// Where a page-mount Operation is judged: what the question of
    /// authorization asks about a mount or an unmount.
    pub fn place_page_mount(&self) -> &PlacePageMount {
        &self.place_page_mount
    }

    /// The version check, on demand, periodic, or right after it was turned on.
    pub fn check_for_update(&self) -> &CheckForUpdate {
        &self.check_for_update
    }

    /// The instance's Public URL, read fresh (`stored → env → none`).
    pub async fn public_url(&self) -> Option<String> {
        self.public_url.resolve().await
    }

    /// The persistent-service health observed at boot.
    pub fn service_health(&self) -> &ServiceHealth {
        &self.service_health
    }

    /// Check once now, detached and panic-isolated — what turning the
    /// `update_check` setting back on triggers.
    pub fn check_for_update_in_background(self: &Arc<Self>) {
        let platform = self.clone();
        adapters::update_schedule::spawn_isolated("update check (re-enabled)", async move {
            platform.check_for_update.now().await;
        });
    }

    /// Start the version check: load the journal's cache so `GET /update`
    /// answers from it at once, then — when `periodic` — the boot check and the
    /// 6 h loop. Detached: a slow release source never delays the daemon.
    pub fn start_update_checks(self: &Arc<Self>, periodic: bool) {
        adapters::update_schedule::start(self.clone(), periodic);
    }
}
