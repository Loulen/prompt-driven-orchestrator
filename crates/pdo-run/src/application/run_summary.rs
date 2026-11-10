//! The Résumés de Run (ADR-0084), rebuilt whole from the event log, the only
//! source of truth, when they are not the log's: before the first boot that
//! keeps them, after a change of their shape, after a daemon that did not keep
//! them wrote to the log. Then every append keeps them
//! ([`EventStore::append`]).

use anyhow::Result;
use tracing::{info, warn};

use crate::application::ports::EventStore;
use crate::domain::run_summary::summarize;

/// Rebuild the Résumés unless they are current. Whether it rebuilt.
///
/// Runs at boot, before the daemon serves: an append between the read of a
/// log and the replacement would be lost from its Résumé.
pub async fn ensure_summaries(store: &dyn EventStore) -> Result<bool> {
    if store.summaries_current().await? {
        return Ok(false);
    }
    let count = rebuild_summaries(store).await?;
    info!("run summaries rebuilt from the event log ({count} runs)");
    Ok(true)
}

/// Replace every Résumé by the one its Run's log gives. How many Runs it
/// summarized. A log that cannot be read is left out (logged).
pub async fn rebuild_summaries(store: &dyn EventStore) -> Result<usize> {
    let mut summaries = Vec::new();
    for run_id in store.run_ids().await? {
        match store.events(&run_id).await {
            Ok(events) => summaries.extend(summarize(&events)),
            Err(e) => warn!("the log of run {run_id} cannot be read: {e:#}"),
        }
    }
    let count = summaries.len();
    store.replace_summaries(summaries).await?;
    Ok(count)
}
