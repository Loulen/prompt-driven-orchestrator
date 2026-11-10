//! Node-transition events, dispatched by kind to the three appliers of the
//! node lifecycle: its start, its completion, its incidents.

use super::{node_completion, node_incident, node_start};
use crate::domain::event::{Event, EventKind};
use crate::domain::state::*;

/// Node-transition events: the per-iteration lifecycle (waiting -> started ->
/// completed/failed/...), plus stop/stale/invalidate and the frontmatter-retry
/// counter. Node-level status derives from the LATEST iteration (see the
/// `NodeFailed` #196/#212 guard).
pub(super) fn apply_node_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::NodeWaiting => node_start::node_waiting(state, event),
        EventKind::NodeStarted => node_start::node_started(state, event),
        EventKind::NodeCompletionReleased => {
            node_completion::node_completion_released(state, event)
        }
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
            node_completion::node_completed(state, event)
        }
        EventKind::NodeAwaitingUser => node_completion::node_awaiting_user(state, event),
        EventKind::NodeResumed => node_completion::node_resumed(state, event),
        EventKind::NodeFailed => node_incident::node_failed(state, event),
        EventKind::NodeInterrupted => node_incident::node_interrupted(state, event),
        EventKind::NodeStopped => node_incident::node_stopped(state, event),
        EventKind::NodeStale => node_incident::node_stale(state, event),
        EventKind::NodeInvalidated => node_incident::node_invalidated(state, event),
        EventKind::FrontmatterRetryPending => {
            node_incident::frontmatter_retry_pending(state, event)
        }
        EventKind::NodeDelivered => node_incident::node_delivered(state, event),
        _ => {}
    }
}
