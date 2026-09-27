//! A node fails, is interrupted, stopped, goes stale, is invalidated, retries
//! its frontmatter, or delivers onto the Run's branch.

use crate::domain::event::Event;
use crate::domain::state::*;

pub(super) fn node_failed(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            let iter = event.iter.unwrap_or(node.iter);
            // Node-level status derives from the LATEST iteration:
            // failing an older iter (e.g. kill_node on a stale
            // iter, #196 via #212) must not mislabel a node whose
            // newer iteration is still live.
            if iter >= node.iter {
                node.status = NodeStatus::Failed;
                node.completed_at = Some(event.ts.clone());
                if let Some(ref payload) = event.payload {
                    node.failure_reason = payload
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    // Read BOTH shapes of the validation evidence: the
                    // after-retry branch puts `violations` at the top level,
                    // the `script` fail-fast branch nests them under `detail`.
                    // Don't flatten the producer — the nesting is what keeps a
                    // fail-fast audit trail distinguishable from an after-retry
                    // one (ADR-0035 §5).
                    //
                    // Collision-checked: the only other payload carrying
                    // `detail` is `MergeConflictDetected`, which routes to
                    // `apply_merge_event` and never reaches this arm.
                    let detail = payload.get("detail");
                    if let Some(arr) = payload
                        .get("violations")
                        .or_else(|| detail.and_then(|d| d.get("violations")))
                        .and_then(|v| v.as_array())
                    {
                        node.frontmatter_violations = arr.clone();
                    }
                    if let Some(arr) = payload
                        .get("missing")
                        .or_else(|| detail.and_then(|d| d.get("missing")))
                        .and_then(|v| v.as_array())
                    {
                        node.missing_outputs = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                }
            }
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = NodeStatus::Failed;
                it.completed_at = Some(event.ts.clone());
            }
        }
    }
}

pub(super) fn node_interrupted(state: &mut RunState, event: &Event) {
    // An infra incident (ADR-0049). Unlike `NodeFailed` this must be
    // visible even when the spawn aborted BEFORE `NodeStarted` opened an
    // iteration (ADR-0050 §1: a spawn abort names its node), so the node
    // is materialised if absent — with no iteration row, which is the
    // honest shape of "the session never started".
    if let Some(ref node_id) = event.node_id {
        let iter = event.iter.unwrap_or(1);
        let reason = event
            .payload
            .as_ref()
            .and_then(|p| p.get("reason"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let node = state
            .nodes
            .entry(node_id.clone())
            .or_insert_with(|| NodeState {
                harness: None,
                isolated_worktree: None,
                skills: None,
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                cost: None,
                node_id: node_id.clone(),
                status: NodeStatus::Interrupted,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            });
        // Node-level status derives from the LATEST iteration, mirroring
        // the `NodeFailed` #196/#212 guard: interrupting an older iter
        // must not mislabel a node whose newer iteration is still live.
        if iter >= node.iter {
            node.status = NodeStatus::Interrupted;
            node.iter = iter;
            node.failure_reason = reason;
            // Not a green completion: leave `completed_at` untouched.
            // Carry the same validation evidence a `NodeFailed` would
            // (#490): an output-validation interrupt should render the red
            // banner with the missing ports / frontmatter violations, not
            // an empty list. Both shapes read, exactly like `NodeFailed`.
            if let Some(ref payload) = event.payload {
                let detail = payload.get("detail");
                if let Some(arr) = payload
                    .get("violations")
                    .or_else(|| detail.and_then(|d| d.get("violations")))
                    .and_then(|v| v.as_array())
                {
                    node.frontmatter_violations = arr.clone();
                }
                if let Some(arr) = payload
                    .get("missing")
                    .or_else(|| detail.and_then(|d| d.get("missing")))
                    .and_then(|v| v.as_array())
                {
                    node.missing_outputs = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
            }
        }
        if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
            it.status = NodeStatus::Interrupted;
        }
    }
}

pub(super) fn node_stopped(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            node.status = NodeStatus::Stopped;
            node.completed_at = Some(event.ts.clone());
            if let Some(ref payload) = event.payload {
                node.failure_reason = payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            let iter = event.iter.unwrap_or(node.iter);
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = NodeStatus::Stopped;
                it.completed_at = Some(event.ts.clone());
            }
        }
    }
}

pub(super) fn node_stale(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            node.status = NodeStatus::Stale;
            let iter = event.iter.unwrap_or(node.iter);
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = NodeStatus::Stale;
            }
        }
    }
}

pub(super) fn node_invalidated(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        state.nodes.remove(node_id);
    }
}

pub(super) fn frontmatter_retry_pending(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            node.frontmatter_retries += 1;
        }
    }
}

/// #654 / ADR-0060: what the delivery put on the Run's branch. Read from
/// the payload rather than re-derived, because the tips are the only trace
/// of a non-isolated NodeRun's contribution — it owns no branch to diff.
/// The last delivery for the node wins: a re-run of the same node delivers
/// again, and the diff must show the latest one.
pub(super) fn node_delivered(state: &mut RunState, event: &Event) {
    if let (Some(node_id), Some(payload)) = (&event.node_id, &event.payload) {
        if let Some(node) = state.nodes.get_mut(node_id) {
            if let (Some(before), Some(after)) = (
                payload.get("before").and_then(|v| v.as_str()),
                payload.get("after").and_then(|v| v.as_str()),
            ) {
                node.delivery = Some(NodeDelivery {
                    before: before.to_string(),
                    after: after.to_string(),
                });
            }
        }
    }
}
