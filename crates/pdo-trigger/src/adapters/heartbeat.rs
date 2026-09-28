//! The heartbeat: the clock that ticks the Triggers, a primary adapter. Every
//! [`TICK_INTERVAL_SECS`] it runs one [`Triggers::tick`], panic-isolated.
//! Best-effort: it only runs while the daemon lives, forward-only — no backfill
//! of missed slots.

use std::sync::Arc;
use std::time::Duration;

use pdo_run::application::supervision::run_isolated;

use crate::domain::schedule::TICK_INTERVAL_SECS;
use crate::Triggers;

/// One tick under panic isolation. The background loop *and* the daemon's test
/// seam both go through here, so production and the synchronous test path
/// behave identically.
pub async fn tick_supervised(triggers: &Arc<Triggers>) {
    let triggers = triggers.clone();
    run_isolated("trigger scheduler", async move { triggers.tick().await }).await;
}

/// Start the heartbeat. Keep the handle alive for as long as the daemon serves.
pub fn spawn(triggers: Arc<Triggers>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(TICK_INTERVAL_SECS));
        loop {
            tick.tick().await;
            tick_supervised(&triggers).await;
        }
    })
}
