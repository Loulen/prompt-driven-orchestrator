//! The price sync (ADR-0034): the operator's « Sync costs », and the refresh
//! of an existing, stale fetched tier at boot. Both take [`PriceSyncLock`]:
//! one sync at a time.

pub mod refresh;
pub mod sync;

use std::sync::Arc;

/// The lock a price sync holds for its whole run.
pub type PriceSyncLock = Arc<tokio::sync::Mutex<()>>;

/// Where the prices come from: the per-daemon override (the layer-3/5 seam),
/// else the shipped default.
#[derive(Debug, Clone)]
pub struct PriceSourceUrl(pub Option<String>);

impl PriceSourceUrl {
    pub fn url(&self) -> String {
        self.0
            .clone()
            .unwrap_or_else(|| crate::domain::price_sync::PRICE_SOURCE_URL_DEFAULT.to_string())
    }
}
