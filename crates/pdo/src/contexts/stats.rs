//! The `stats` context: the run context's event log (its `EventStore` and its
//! cross-Run reads over SQLite), the absorptions over SQLite, the Runs' files
//! and transcripts on disk, the price files of the host home and models.dev,
//! the Projets of the workspace, the platform's audit journal, the kernel's
//! bus, and the Triggers' fires, read in the trigger context's tables.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::Platform;
use pdo_run::adapters::sqlite_event_store::SqliteEventStore;
use pdo_stats::adapters::contexts::{
    BusAnnouncements, PlatformAudit, WorkspaceProjects as StatsProjects,
};
use pdo_stats::adapters::models_dev::HttpPriceSource;
use pdo_stats::adapters::price_files::HomePriceFiles;
use pdo_stats::adapters::run_events::RunContextEvents;
use pdo_stats::adapters::run_files::DiskRunFiles;
use pdo_stats::adapters::sqlite_absorptions::SqliteAbsorptions;
use pdo_stats::adapters::transcripts::HarnessTranscripts;
use pdo_stats::{Stats, StatsOptions, StatsPorts};
use pdo_workspace::application::Workspace;

use super::ContextResources;
use crate::ports::trigger_fires::TriggerTableFires;

pub(super) fn stats(
    r: &ContextResources,
    platform: &Arc<Platform>,
    workspace: &Arc<Workspace>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Stats> {
    let store = Arc::new(SqliteEventStore::new(r.db.clone()));
    let home_override = r.sandbox_home_override.clone();
    Stats::new(
        StatsPorts {
            events: Arc::new(RunContextEvents::new(store.clone(), store)),
            triggers: Arc::new(TriggerTableFires { db: r.db.clone() }),
            projects: Arc::new(StatsProjects(workspace.clone())),
            absorptions: Arc::new(SqliteAbsorptions::new(r.db.clone())),
            run_files: Arc::new(DiskRunFiles::new(
                r.repo_root.clone(),
                home_override.clone(),
            )),
            transcripts: Arc::new(HarnessTranscripts),
            price_files: Arc::new(HomePriceFiles::new(home_override)),
            price_source: Arc::new(HttpPriceSource),
            audit: Arc::new(PlatformAudit(platform.clone())),
            announcements: Arc::new(BusAnnouncements(r.bus.clone())),
            extensions: host,
        },
        StatsOptions {
            repo_root: r.repo_root.clone(),
            price_source_url: r.price_source_url.clone(),
            price_refresh_at_boot: r.price_refresh_at_boot,
        },
    )
}
