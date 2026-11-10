//! A node completes (or is auto-completed), declares a wait, is released or
//! resumed.

use crate::domain::event::Event;
use crate::domain::state::*;

pub(super) fn node_completion_released(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            let iter = event.iter.unwrap_or(node.iter);
            if iter == node.iter {
                node.status = NodeStatus::Running;
                node.awaiting = None;
            }
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = NodeStatus::Running;
                it.completion_released = true;
            }
        }
    }
}

pub(super) fn node_completed(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        // #600: a **skip** (`skip_node` / reachability auto-skip) can
        // complete a node that never started — that is the whole point of
        // skipping a node stuck waiting on an input that never came. Create
        // it directly as terminal, with no transient session-less `Running`
        // window for the liveness sweep to flag; it then counts as satisfied
        // for re-projection (a reopen never re-spawns it, FP #4).
        let is_skip = event
            .payload
            .as_ref()
            .and_then(|p| p.get("skipped"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // #620: the discriminator is "never ran", NOT the skip source. A
        // reachability auto-skip prunes a never-started node (→ `Skipped`,
        // greyed with its reason); a graceful `skip_node` skips a node that
        // DID start and reach a decision (→ stays `Completed`, and the run,
        // not the node, carries the "nothing to do" signal).
        // A skip of an iteration already settled is a duplicate — two sweeps
        // that pruned the same node from one snapshot, both appended: the
        // transition guard no-ops it, unless both appends raced its check. It
        // must not turn a pruned node `Completed`.
        if is_skip
            && state.nodes.get(node_id).is_some_and(|n| {
                n.iter == event.iter.unwrap_or(n.iter) && n.status.is_settled_complete()
            })
        {
            return;
        }
        let never_started_skip = is_skip && !state.nodes.contains_key(node_id);
        // A never-started node the reachability sweep settled is **pruned**:
        // dead for routing (ADR-0082). The operator's skip of a never-started
        // node (`skip_node`) is not: it settles the node as if it had run.
        let pruned = never_started_skip
            && event
                .payload
                .as_ref()
                .and_then(|p| p.get("source"))
                .and_then(|v| v.as_str())
                == Some(AUTO_SKIP_UNREACHABLE_SOURCE);
        let done_status = if never_started_skip {
            NodeStatus::Skipped
        } else {
            NodeStatus::Completed
        };
        let skip_reason = if never_started_skip {
            event
                .payload
                .as_ref()
                .and_then(|p| p.get("reason"))
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            None
        };
        if never_started_skip {
            let iter = event.iter.unwrap_or(1);
            state.nodes.insert(
                node_id.clone(),
                NodeState {
                    harness: None,
                    isolated_worktree: None,
                    skills: None,
                    missing_skills: Vec::new(),
                    skipped_skills: Vec::new(),
                    cost: None,
                    node_id: node_id.clone(),
                    status: done_status.clone(),
                    iter,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                    failure_reason: None,
                    skip_reason: skip_reason.clone(),
                    pruned,
                    iterations: vec![IterationInfo {
                        iter,
                        status: done_status.clone(),
                        started_at: Some(event.ts.clone()),
                        completed_at: Some(event.ts.clone()),
                        interactive: false,
                        completion_released: false,
                        waited_ms: 0,
                        open_wait_since: None,
                    }],
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                    missing_outputs: Vec::new(),
                    delivery: None,
                    awaiting: None,
                },
            );
        }
        if let Some(node) = state.nodes.get_mut(node_id) {
            node.status = done_status.clone();
            node.completed_at = Some(event.ts.clone());
            if never_started_skip {
                node.skip_reason = skip_reason.clone();
            }
            let iter = event.iter.unwrap_or(node.iter);
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = done_status.clone();
                it.completed_at = Some(event.ts.clone());
            }
        }
    }
}

pub(super) fn node_awaiting_user(state: &mut RunState, event: &Event) {
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            node.status = NodeStatus::AwaitingUser;
            let iter = event.iter.unwrap_or(node.iter);
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                it.status = NodeStatus::AwaitingUser;
            }
            // #588: the declared wait's cause and message. A legacy
            // payload-less event (pre-#588 spawn wait) reads as
            // `declared` without a message; the binding marker
            // (ADR-0064) keeps its `reason` slug as the cause.
            let payload = event.payload.as_ref();
            let cause = payload
                .and_then(|p| p.get("cause").or_else(|| p.get("reason")))
                .and_then(|v| v.as_str())
                .unwrap_or(AWAITING_CAUSE_DECLARED)
                .to_string();
            let message = payload
                .and_then(|p| p.get("message"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            node.awaiting = Some(AwaitingInfo {
                cause,
                message,
                since: event.ts.clone(),
                child_run_id: None,
            });
        }
    }
}

pub(super) fn node_resumed(state: &mut RunState, event: &Event) {
    // #588 / ADR-0069: a human keystroke lifted the wait. Only a node
    // that IS awaiting on its current iteration moves; anything else
    // (a stale resume racing a completion) is a no-op on projection.
    if let Some(ref node_id) = event.node_id {
        if let Some(node) = state.nodes.get_mut(node_id) {
            let iter = event.iter.unwrap_or(node.iter);
            if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                if it.status == NodeStatus::AwaitingUser {
                    it.status = NodeStatus::Running;
                }
            }
            if iter == node.iter && node.status == NodeStatus::AwaitingUser {
                node.status = NodeStatus::Running;
                node.awaiting = None;
            }
        }
    }
}
