//! Every Run of the log, projected, with the child-awaiting overlay applied
//! (#588 / ADR-0069 §4): a Run whose descendant waits on its user reads as
//! awaiting too, derived on read and never written.

use crate::application::ports::EventStore;
use crate::domain::child_awaiting;
use crate::domain::projection::project;
use crate::domain::state::{RunState, RunStatus};

/// Project every Run of `store` and apply the overlay. A Run whose log cannot
/// be read is left out.
pub async fn project_all_overlaid(store: &dyn EventStore) -> Vec<RunState> {
    let Ok(run_ids) = store.run_ids().await else {
        return Vec::new();
    };
    let mut states = Vec::new();
    for run_id in run_ids {
        let Ok(events) = store.events(&run_id).await else {
            continue;
        };
        if let Some(run_state) = project(&events) {
            states.push(run_state);
        }
    }
    child_awaiting::overlay(&mut states);
    states
}

/// Replace `run` by its overlaid twin when it has a descendant awaiting its
/// user. A Run with no child at all is left untouched without projecting the
/// store, and so is one whose Résumés (ADR-0084) show no descendant awaiting:
/// the common cases stay cheap, only an actual lift projects every Run (for
/// the node banner, which needs the child's nodes).
pub async fn overlay_child_awaiting(store: &dyn EventStore, run: &mut RunState) {
    if run.status != RunStatus::Running {
        return;
    }
    if !store.has_children(&run.run_id).await {
        return;
    }
    if let Ok(mut summaries) = store.summaries().await {
        child_awaiting::overlay(&mut summaries);
        let lifted = summaries
            .iter()
            .find(|summary| summary.run_id == run.run_id)
            .is_some_and(|summary| summary.status == RunStatus::AwaitingUser);
        if !lifted {
            return;
        }
    }
    let overlaid = project_all_overlaid(store).await;
    if let Some(found) = overlaid.into_iter().find(|r| r.run_id == run.run_id) {
        *run = found;
    }
}
