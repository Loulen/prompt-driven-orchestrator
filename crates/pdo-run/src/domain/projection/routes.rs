//! What the manager's commands add up to per region: the bounded regions'
//! bumps and ends, and the cycles' extensions.

use std::collections::HashMap;

use crate::domain::event::{Event, EventKind};

/// The folded manager routing applied to one bounded loop region by id
/// (ADR-0011 / #152). The Pipeline Manager can route an exhausted-unrouted
/// region: **bump** it (run `bumped_by` more iterations) or **end** it (fire its
/// completion). Both are issued as `CommandIssued` events; this is their
/// projection onto a single region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionRoute {
    /// Extra iterations the manager added on top of the region's `max_iter`
    /// (sum of every `bump_region` command for this id).
    pub bumped_by: i64,
    /// True once the manager ended the region (any `end_region` command for this
    /// id), so the scheduler stops blocking it "exhausted — unrouted".
    pub ended: bool,
}

/// Folds the manager's loop-region routing commands (ADR-0011 / #152) per region
/// id from the event log: `bump_region` accumulates `additional_iter`,
/// `end_region` flips `ended`. The result drives `resume_run` continuation of an
/// exhausted-unrouted region without restarting the daemon.
pub fn collect_region_routes(events: &[Event]) -> HashMap<String, RegionRoute> {
    let mut routes: HashMap<String, RegionRoute> = HashMap::new();
    for event in events {
        if event.kind != EventKind::CommandIssued {
            continue;
        }
        let Some(ref payload) = event.payload else {
            continue;
        };
        let cmd = payload.get("command").and_then(|v| v.as_str());
        let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) else {
            continue;
        };
        match cmd {
            Some("bump_region") => {
                let additional = payload
                    .get("additional_iter")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                routes.entry(region_id.to_string()).or_default().bumped_by += additional;
            }
            Some("end_region") => {
                routes.entry(region_id.to_string()).or_default().ended = true;
            }
            _ => {}
        }
    }
    routes
}

pub fn collect_cycle_extensions(events: &[Event]) -> HashMap<String, i64> {
    let mut extensions: HashMap<String, i64> = HashMap::new();
    for event in events {
        if event.kind != EventKind::CommandIssued {
            continue;
        }
        if let Some(ref payload) = event.payload {
            let cmd = payload.get("command").and_then(|v| v.as_str());
            if cmd == Some("extend_cycle") {
                if let (Some(node_id), Some(additional)) = (
                    payload.get("node_id").and_then(|v| v.as_str()),
                    payload.get("additional_iter").and_then(|v| v.as_i64()),
                ) {
                    *extensions.entry(node_id.to_string()).or_insert(0) += additional;
                }
            }
        }
    }
    extensions
}
