//! The boot refresh of the fetched price tier (ADR-0034): if — and only if — it
//! already exists and is stale. A transition of the daemon, not a gesture: it
//! never asks the Autorisation d'opération (D10).
//!
//! Three gates, in this order, each load-bearing:
//! 1. the config flag;
//! 2. **`fetched.json` must already exist** — the boot pass REFRESHES, it never
//!    seeds, so there is no egress before the user's first explicit click
//!    (which also keeps the integration daemons, booting from a fresh `HOME`,
//!    offline);
//! 3. it must be older than [`PRICE_REFRESH_MAX_AGE`].
//!
//! A failure is a `warn!`, never fatal: a daemon restarted by systemd can come
//! up before DNS is ready, and a boot must not fail on that.

use std::sync::Arc;

use super::{PriceSourceUrl, PriceSyncLock};
use crate::application::ports::{PriceFiles, PriceSource};
use crate::domain::price_sync::{normalize_models_dev, PRICE_REFRESH_MAX_AGE};

pub struct RefreshPrices {
    files: Arc<dyn PriceFiles>,
    source: Arc<dyn PriceSource>,
    url: PriceSourceUrl,
    lock: PriceSyncLock,
    enabled: bool,
}

impl RefreshPrices {
    pub fn new(
        files: Arc<dyn PriceFiles>,
        source: Arc<dyn PriceSource>,
        url: PriceSourceUrl,
        lock: PriceSyncLock,
        enabled: bool,
    ) -> Self {
        Self {
            files,
            source,
            url,
            lock,
            enabled,
        }
    }

    pub async fn execute(&self) {
        if !self.enabled {
            return;
        }
        let Ok(home_root) = self.files.home() else {
            return; // no HOME → no path → nothing to refresh, silently
        };
        let (_, fetched_path) = self.files.paths(&home_root);
        if !fetched_path.exists() {
            // The user has never synced; asking the network now would be an
            // egress they did not consent to.
            return;
        }
        let table = self.files.load(&home_root);
        if let Some(fetched_at) = table.fetched_at() {
            if let Ok(then) = chrono::DateTime::parse_from_rfc3339(fetched_at) {
                let age =
                    chrono::Utc::now().signed_duration_since(then.with_timezone(&chrono::Utc));
                if age
                    < chrono::Duration::from_std(PRICE_REFRESH_MAX_AGE)
                        .unwrap_or_else(|_| chrono::Duration::hours(24))
                {
                    return; // fresh → noop
                }
            }
            // An unparseable `fetched_at` falls through and refreshes: a cache
            // whose vintage cannot be read is one worth re-fetching.
        }

        // Same lock as the button. Losing the race means a manual sync is
        // running, which does strictly better — give up without a word.
        let Ok(_guard) = self.lock.try_lock() else {
            return;
        };
        let url = self.url.url();
        let body = match self.source.fetch(&url).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    "price refresh at boot: {url} unreachable ({e}) — keeping the current table"
                );
                return;
            }
        };
        let fresh = match normalize_models_dev(&body) {
            Ok((rows, _)) => rows,
            Err(e) => {
                tracing::warn!(
                    "price refresh at boot: {url} unusable ({e}) — keeping the current table"
                );
                return;
            }
        };
        if fresh == *table.fetched_prices() {
            return; // nothing moved: leave the file, and the memo, alone
        }
        match self
            .files
            .write_fetched(&fetched_path, &url, &pdo_kernel::clock::now_iso(), &fresh)
        {
            Ok(()) => tracing::info!("price refresh at boot: {} row(s) from {url}", fresh.len()),
            Err(e) => tracing::warn!(
                "price refresh at boot: cannot write {}: {e}",
                fetched_path.display()
            ),
        }
    }
}
