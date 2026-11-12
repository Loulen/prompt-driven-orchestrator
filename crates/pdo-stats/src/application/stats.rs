//! [`Stats`]: the context's use cases, wired over its ports.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;

use super::absorptions::combine::CombineAbsorption;
use super::absorptions::list::ListAbsorptions;
use super::absorptions::record_rename::RecordRename;
use super::absorptions::uncombine::UncombineMember;
use super::absorptions::AbsorptionWrites;
use super::performance::ReadPerformance;
use super::ports::{
    AbsorptionStore, Announcements, AuditJournal, PriceFiles, PriceSource, Projects, RunEvents,
    RunFiles, SessionTranscripts, TriggerFires,
};
use super::prices::refresh::RefreshPrices;
use super::prices::sync::SyncPrices;
use super::prices::{PriceSourceUrl, PriceSyncLock};
use super::read_cost::ReadCost;
use super::read_overview::ReadOverview;

/// The adapters the composition root plugs into the context.
pub struct StatsPorts {
    pub events: Arc<dyn RunEvents>,
    pub triggers: Arc<dyn TriggerFires>,
    pub projects: Arc<dyn Projects>,
    pub absorptions: Arc<dyn AbsorptionStore>,
    pub run_files: Arc<dyn RunFiles>,
    pub transcripts: Arc<dyn SessionTranscripts>,
    pub price_files: Arc<dyn PriceFiles>,
    pub price_source: Arc<dyn PriceSource>,
    pub audit: Arc<dyn AuditJournal>,
    pub announcements: Arc<dyn Announcements>,
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The daemon's knobs the context reads.
#[derive(Debug, Clone, Default)]
pub struct StatsOptions {
    /// The daemon's repo root: the Projet of a Run that names no repository.
    pub repo_root: PathBuf,
    /// Points the price sync at another source (the layer-3/5 test seam).
    pub price_source_url: Option<String>,
    /// Whether the boot refreshes a stale fetched tier (`PDO_PRICE_SYNC`).
    pub price_refresh_at_boot: bool,
}

/// The stats context.
pub struct Stats {
    read_overview: ReadOverview,
    read_cost: ReadCost,
    read_performance: ReadPerformance,
    list_absorptions: ListAbsorptions,
    combine: CombineAbsorption,
    uncombine: UncombineMember,
    record_rename: RecordRename,
    sync_prices: SyncPrices,
    refresh_prices: RefreshPrices,
}

impl Stats {
    pub fn new(ports: StatsPorts, options: StatsOptions) -> Arc<Self> {
        let writes: AbsorptionWrites = Arc::default();
        let price_lock: PriceSyncLock = Arc::default();
        let url = PriceSourceUrl(options.price_source_url);
        Arc::new(Self {
            read_overview: ReadOverview::new(
                ports.events.clone(),
                ports.triggers,
                ports.absorptions.clone(),
                ports.extensions.clone(),
            ),
            read_cost: ReadCost::new(
                ports.events.clone(),
                ports.absorptions.clone(),
                ports.projects,
                ports.run_files.clone(),
                options.repo_root,
                ports.extensions.clone(),
            ),
            read_performance: ReadPerformance::new(
                ports.events.clone(),
                ports.absorptions.clone(),
                ports.run_files,
                ports.transcripts,
                ports.extensions.clone(),
            ),
            list_absorptions: ListAbsorptions::new(ports.absorptions.clone()),
            combine: CombineAbsorption::new(
                ports.absorptions.clone(),
                ports.events.clone(),
                ports.audit.clone(),
                ports.extensions.clone(),
                writes.clone(),
            ),
            uncombine: UncombineMember::new(
                ports.absorptions.clone(),
                ports.events,
                ports.audit,
                ports.extensions.clone(),
                writes.clone(),
            ),
            record_rename: RecordRename::new(ports.absorptions, writes),
            sync_prices: SyncPrices::new(
                ports.price_files.clone(),
                ports.price_source.clone(),
                ports.announcements,
                ports.extensions,
                url.clone(),
                price_lock.clone(),
            ),
            refresh_prices: RefreshPrices::new(
                ports.price_files,
                ports.price_source,
                url,
                price_lock,
                options.price_refresh_at_boot,
            ),
        })
    }

    pub fn read_overview(&self) -> &ReadOverview {
        &self.read_overview
    }

    pub fn read_cost(&self) -> &ReadCost {
        &self.read_cost
    }

    pub fn read_performance(&self) -> &ReadPerformance {
        &self.read_performance
    }

    pub fn list_absorptions(&self) -> &ListAbsorptions {
        &self.list_absorptions
    }

    pub fn combine(&self) -> &CombineAbsorption {
        &self.combine
    }

    pub fn uncombine(&self) -> &UncombineMember {
        &self.uncombine
    }

    /// The absorption a Pipeline rename leaves behind (the pipeline context's
    /// `RenameLedger`).
    pub fn record_rename(&self) -> &RecordRename {
        &self.record_rename
    }

    pub fn sync_prices(&self) -> &SyncPrices {
        &self.sync_prices
    }

    /// The boot refresh of a stale fetched tier. Production spawns it detached
    /// once the daemon listens; a test drives it directly.
    pub fn refresh_prices(&self) -> &RefreshPrices {
        &self.refresh_prices
    }
}
