//! The live NodeRun sessions across every Run, and the live Sessions
//! hébergées: what the session cap counts.

use tracing::warn;

use crate::application::events::load_all_run_ids;
use crate::application::events::load_events;
use crate::domain::admission;
use crate::domain::projection;
use crate::Runs;

/// Count the live NodeRun sessions across *all* runs, plus the live Sessions
/// hébergées (ADR-0091), for admission control. Pipeline Manager sessions are
/// not nodes, so they are excluded by construction.
pub async fn count_global_live_sessions(db: &Runs) -> usize {
    count_global_live_sessions_excluding(db, None).await
}

/// [`count_global_live_sessions`], minus the one slot the calling spawn is taking
/// back. Only `spawn_node`'s admission gate may pass an exclusion: every other
/// reader must call the plain form, because an observability endpoint has to report
/// the **true** count.
pub async fn count_global_live_sessions_excluding(
    db: &Runs,
    exclude: Option<admission::SlotExclusion<'_>>,
) -> usize {
    let run_ids = match load_all_run_ids(db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("admission: failed to load run ids: {e}");
            return 0;
        }
    };
    let mut states = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        if let Ok(events) = load_events(db, &run_id).await {
            if let Some(state) = projection::project(&events) {
                states.push(state);
            }
        }
    }
    admission::count_live_node_sessions_excluding(states.iter(), exclude)
        + db.sessions().hosted_live()
}
