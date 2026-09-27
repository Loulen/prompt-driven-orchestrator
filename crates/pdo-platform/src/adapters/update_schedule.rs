//! The periodic version check: the driving adapter that runs the check at boot
//! and every [`UPDATE_CHECK_INTERVAL`], detached and panic-isolated.

use std::future::Future;
use std::sync::Arc;

use crate::domain::update_check::UPDATE_CHECK_INTERVAL;
use crate::Platform;

/// Load the cache, then — when `periodic` — the boot check and the loop.
pub(crate) fn start(platform: Arc<Platform>, periodic: bool) {
    tokio::spawn(async move {
        platform.check_for_update.load().await;
        if !periodic {
            return;
        }
        let mut ticker = tokio::time::interval(UPDATE_CHECK_INTERVAL);
        loop {
            ticker.tick().await; // fires immediately: this IS the boot check
            let p = platform.clone();
            isolated("update check", async move {
                p.check_for_update.periodic().await;
            })
            .await;
        }
    });
}

/// Run `fut` detached, a panic in it contained and logged.
pub(crate) fn spawn_isolated<F>(label: &'static str, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(isolated(label, fut));
}

async fn isolated<F>(label: &str, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Err(e) = tokio::spawn(fut).await {
        if e.is_panic() {
            tracing::warn!("{label}: tick panicked, isolated — next tick recovers: {e}");
        }
    }
}
