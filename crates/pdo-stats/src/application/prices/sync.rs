//! « Sync costs » (`POST /settings/cost-prices/sync`): fetch the price source
//! and rewrite the fetched tier. Asks the Autorisation d'opération first.
//!
//! NOT a `POST /runs/:id/commands` kind: that dispatcher is run-scoped, and a
//! price sync touches neither the event log nor a Run projection. NOT under
//! `/stats` either — ADR-0029 makes that a read-derivation surface.
//!
//! An explicitly requested effect that fails is a HARD error that NAMES the
//! source, never a silent fallback (ADR-0030); a table that did not move is
//! said (`noop: true` + `reason`), never a blind `{ok: true}` (ADR-0025).

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::{PriceSourceUrl, PriceSyncLock};
use crate::application::ports::{Announcements, PriceFiles, PriceSource};
use crate::domain::price_sync::{diff_price_sync, normalize_models_dev};

/// Why the sync did not happen. Nothing is written in any of these cases.
#[derive(Debug)]
pub enum SyncError {
    Refused(Denied),
    /// No home for the price files, or the write failed: `500`.
    Failed(String),
    /// A sync is already in flight: `409`.
    InFlight,
    /// The source is unreachable or unusable: `502`.
    Source(String),
}

pub struct SyncPrices {
    files: Arc<dyn PriceFiles>,
    source: Arc<dyn PriceSource>,
    announcements: Arc<dyn Announcements>,
    host: Arc<dyn ExtensionHost>,
    url: PriceSourceUrl,
    lock: PriceSyncLock,
}

impl SyncPrices {
    pub fn new(
        files: Arc<dyn PriceFiles>,
        source: Arc<dyn PriceSource>,
        announcements: Arc<dyn Announcements>,
        host: Arc<dyn ExtensionHost>,
        url: PriceSourceUrl,
        lock: PriceSyncLock,
    ) -> Self {
        Self {
            files,
            source,
            announcements,
            host,
            url,
            lock,
        }
    }

    /// Sync for `actor`. The report is the JSON the route answers.
    pub async fn execute(&self, actor: &Actor) -> Result<serde_json::Value, SyncError> {
        let operation = Operation::new(ResourceKind::PriceTable, Verb::Action("sync".into()));
        authorize(self.host.as_ref(), actor, &operation).map_err(SyncError::Refused)?;
        let home_root = self.files.home().map_err(SyncError::Failed)?;
        // `try_lock`, not `.lock()`: a second click brings nothing, and ADR-0025
        // wants it said rather than queued silently.
        let Ok(_guard) = self.lock.try_lock() else {
            return Err(SyncError::InFlight);
        };

        let url = self.url.url();
        let before = self.files.load(&home_root);
        let body = self
            .source
            .fetch(&url)
            .await
            .map_err(|e| SyncError::Source(format!("price source unreachable: {url}: {e}")))?;
        // Includes the empty-harvest guard: an upstream schema drift must NOT be
        // able to write an empty file over the last known table.
        let (fresh, rejected) = normalize_models_dev(&body)
            .map_err(|e| SyncError::Source(format!("price source unusable: {url}: {e}")))?;

        let outcome = diff_price_sync(&before, &fresh);
        let (_, fetched_path) = self.files.paths(&home_root);
        let rejected_msgs: Vec<String> = rejected
            .iter()
            .map(|r| format!("{}: {}", r.key, r.why))
            .collect();

        if !outcome.changed {
            // Nothing to write, so nothing is written — which leaves the table's
            // fingerprint alone and keeps the cost-breakdown memo warm.
            return Ok(serde_json::json!({
                "ok": true,
                "noop": true,
                "reason": format!(
                    "table already up to date — {} row(s) from the source, none changed",
                    outcome.rows
                ),
                "source": url,
                "fetched_at": before.fetched_at(),
                "rows": outcome.rows,
                "added": Vec::<String>::new(),
                "updated": Vec::<String>::new(),
                "unchanged": outcome.unchanged,
                "rejected": rejected_msgs,
                "shadowed_by_manual": outcome.shadowed_by_manual,
            }));
        }

        let fetched_at = pdo_kernel::clock::now_iso();
        self.files
            .write_fetched(&fetched_path, &url, &fetched_at, &fresh)
            .map_err(|e| {
                SyncError::Failed(format!("cannot write {}: {e}", fetched_path.display()))
            })?;

        // Tell every connected client the table moved, so an open Stats modal is
        // not the only thing that knows.
        self.announcements.cost_prices_synced(outcome.rows);

        Ok(serde_json::json!({
            "ok": true,
            "source": url,
            "fetched_at": fetched_at,
            "rows": outcome.rows,
            "added": outcome.added,
            "updated": outcome.updated,
            "unchanged": outcome.unchanged,
            "rejected": rejected_msgs,
            "shadowed_by_manual": outcome.shadowed_by_manual,
        }))
    }
}
