//! Switch routing and the loop, for-each and collection regions.

use super::support::upsert_iteration;
use crate::domain::event::{Event, EventKind};
use crate::domain::state::*;

/// `SwitchRouted`: a switch node both records its chosen branch in
/// `switch_states` AND writes a synthetic `Completed` node entry (the switch has
/// no NodeRun session of its own), so it is kept as its own concern rather than
/// folded into the node applier.
pub(super) fn apply_switch_event(state: &mut RunState, event: &Event) {
    if let Some(ref payload) = event.payload {
        if let Some(node_id) = payload.get("node_id").and_then(|v| v.as_str()) {
            let chosen_branch = payload
                .get("chosen_branch")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();

            state.switch_states.insert(
                node_id.to_string(),
                SwitchState {
                    switch_node_id: node_id.to_string(),
                    chosen_branch: chosen_branch.clone(),
                    evaluated_at: event.ts.clone(),
                },
            );

            let iter = event.iter.unwrap_or(1);
            let node = state
                .nodes
                .entry(node_id.to_string())
                .or_insert_with(|| NodeState {
                    harness: None,
                    isolated_worktree: None,
                    skills: None,
                    missing_skills: Vec::new(),
                    skipped_skills: Vec::new(),
                    cost: None,
                    node_id: node_id.to_string(),
                    status: NodeStatus::Completed,
                    iter,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                    failure_reason: None,
                    skip_reason: None,
                    pruned: false,
                    iterations: Vec::new(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                    missing_outputs: Vec::new(),
                    delivery: None,
                    awaiting: None,
                });
            node.status = NodeStatus::Completed;
            node.completed_at = Some(event.ts.clone());
            node.iter = iter;
            upsert_iteration(
                &mut node.iterations,
                IterationInfo {
                    iter,
                    status: NodeStatus::Completed,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                    interactive: false,
                    completion_released: false,
                    waited_ms: 0,
                    open_wait_since: None,
                },
            );
        }
    }
}

/// Bounded loop-region lap accounting: track the current/max iteration, the
/// break flag, and the done flag, keyed by `loop_node_id`. `LoopMaxReached` is
/// purely informational.
pub(super) fn apply_loop_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::LoopIterStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                    let max_iter = payload
                        .get("max_iter")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(5);
                    let ls = state
                        .loop_states
                        .entry(loop_node_id.to_string())
                        .or_insert_with(|| LoopState {
                            loop_node_id: loop_node_id.to_string(),
                            current_iter: 1,
                            max_iter,
                            break_received: false,
                            done: false,
                        });
                    ls.current_iter = iter;
                    ls.max_iter = max_iter;
                }
            }
        }
        EventKind::LoopBreakReceived => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                        ls.break_received = true;
                    }
                }
            }
        }
        EventKind::LoopMaxReached => {
            // Informational
        }
        EventKind::LoopDone => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                        ls.done = true;
                    }
                }
            }
        }
        _ => {}
    }
}

/// ForEach barrier accounting: track total items, the break flag, and the done
/// flag, keyed by `foreach_node_id`. An empty list short-circuits straight to
/// done.
pub(super) fn apply_foreach_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::ForEachStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    let total_items = payload
                        .get("total_items")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    state
                        .foreach_states
                        .entry(foreach_node_id.to_string())
                        .or_insert_with(|| ForEachState {
                            foreach_node_id: foreach_node_id.to_string(),
                            total_items,
                            break_received: false,
                            done: false,
                        });
                }
            }
        }
        EventKind::ForEachEmpty => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    let fs = state
                        .foreach_states
                        .entry(foreach_node_id.to_string())
                        .or_insert_with(|| ForEachState {
                            foreach_node_id: foreach_node_id.to_string(),
                            total_items: 0,
                            break_received: false,
                            done: false,
                        });
                    fs.done = true;
                }
            }
        }
        EventKind::ForEachBreakReceived => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                        fs.break_received = true;
                    }
                }
            }
        }
        EventKind::ForEachDone => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                        fs.done = true;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Collection-region barrier accounting (ADR-0011 / #269): track the resolved
/// collection size and the done flag, keyed by region id. The region twin of
/// [`apply_foreach_event`]; an empty collection short-circuits straight to
/// done. Panic-free on malformed payloads (missing keys are ignored).
pub(super) fn apply_collection_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::CollectionStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    let total_items = payload
                        .get("total_items")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let entry = payload
                        .get("entry")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    // Pre-#453 payloads carry `entry` but no `members`: fall
                    // back to the entry alone, which is the whole region for the
                    // common single-member collection.
                    let members: Vec<String> = payload
                        .get("members")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_else(|| {
                            if entry.is_empty() {
                                Vec::new()
                            } else {
                                vec![entry.clone()]
                            }
                        });
                    state
                        .collection_states
                        .entry(region_id.to_string())
                        .or_insert_with(|| CollectionState {
                            region_id: region_id.to_string(),
                            total_items,
                            done: false,
                            entry,
                            members,
                        });
                }
            }
        }
        EventKind::CollectionEmpty => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    let cs = state
                        .collection_states
                        .entry(region_id.to_string())
                        .or_insert_with(|| CollectionState {
                            region_id: region_id.to_string(),
                            total_items: 0,
                            done: false,
                            entry: String::new(),
                            members: Vec::new(),
                        });
                    cs.done = true;
                }
            }
        }
        EventKind::CollectionDone => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    if let Some(cs) = state.collection_states.get_mut(region_id) {
                        cs.done = true;
                    }
                }
            }
        }
        _ => {}
    }
}
