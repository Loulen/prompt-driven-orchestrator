//! The absorptions (#888, ADR-0077): their list, the operator's Combine and
//! Uncombine, and the absorption a Pipeline rename leaves behind (#891).
//!
//! A write reads the stored absorptions and the Pipelines' history first, then
//! writes in one transaction; [`AbsorptionWrites`] serializes the writes, so no
//! other write slips between the read and the write.

pub mod combine;
pub mod list;
pub mod record_rename;
pub mod uncombine;

use std::collections::BTreeMap;
use std::sync::Arc;

use super::ports::{AbsorptionStore, RunEvents};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::absorption::scopes::PipelineHistory;

/// The lock every absorption write takes.
pub type AbsorptionWrites = Arc<tokio::sync::Mutex<()>>;

/// The resolver of one Stats read: every stored absorption, or none at all
/// under `uncombined`.
pub async fn load_resolver(
    store: &dyn AbsorptionStore,
    uncombined: bool,
) -> anyhow::Result<AbsorptionResolver> {
    if uncombined {
        return Ok(AbsorptionResolver::default());
    }
    Ok(AbsorptionResolver::from_rows(&store.list().await?))
}

/// What the event log says of each raw Pipeline key of `pipelines`.
pub async fn histories(
    events: &dyn RunEvents,
    pipelines: &[String],
) -> anyhow::Result<BTreeMap<String, PipelineHistory>> {
    let mut histories = BTreeMap::new();
    for pipeline in pipelines {
        if histories.contains_key(pipeline) {
            continue;
        }
        let history = PipelineHistory {
            ran: events.nodes_started_under(pipeline).await?,
            latest: events.latest_run_start(pipeline).await?,
        };
        histories.insert(pipeline.clone(), history);
    }
    Ok(histories)
}
