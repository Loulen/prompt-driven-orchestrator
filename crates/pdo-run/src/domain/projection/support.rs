//! Small folds the appliers share.

use std::collections::HashSet;

use crate::domain::event::{EdgeInfo, NodeDefInfo};
use crate::domain::state::IterationInfo;

pub(super) fn entry_node_ids(edges: &[EdgeInfo], node_defs: &[NodeDefInfo]) -> Vec<String> {
    let start_id = node_defs
        .iter()
        .find(|n| n.node_type == "start")
        .map(|n| n.id.as_str());

    if let Some(start_id) = start_id {
        edges
            .iter()
            .filter(|e| e.source_node == start_id)
            .map(|e| e.target_node.clone())
            .collect()
    } else {
        let nodes_with_unconditional_incoming: HashSet<&str> = edges
            .iter()
            .filter(|e| e.when_clause.is_none())
            .map(|e| e.target_node.as_str())
            .collect();

        node_defs
            .iter()
            .filter(|n| !nodes_with_unconditional_incoming.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }
}

pub(super) fn upsert_iteration(iterations: &mut Vec<IterationInfo>, new: IterationInfo) {
    if let Some(existing) = iterations.iter_mut().find(|i| i.iter == new.iter) {
        existing.status = new.status;
        existing.interactive = new.interactive;
        existing.completion_released = new.completion_released;
        if new.started_at.is_some() {
            existing.started_at = new.started_at;
        }
        if new.completed_at.is_some() {
            existing.completed_at = new.completed_at;
        }
    } else {
        iterations.push(new);
    }
}
