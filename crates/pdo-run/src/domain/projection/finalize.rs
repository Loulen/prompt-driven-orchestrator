//! The reconciliation pass after the fold, and the stall verdict.

use super::reasons::parse_reason_code;
use crate::domain::state::*;

/// Post-fold reconciliation, run once after every event has been applied.
///
/// Two passes that cannot be done per-event because they depend on the whole
/// fold being complete: (1) sort each node's iterations by `iter` and reconcile
/// the node's top-level `iter` to the latest (handles out-of-order events), and
/// (2) derive run-level `AwaitingUser` from node states — a `Running` run with
/// any awaiting node is itself awaiting the user.
pub(super) fn finalize(state: &mut RunState) {
    for node in state.nodes.values_mut() {
        node.iterations.sort_by_key(|i| i.iter);
        if let Some(max_iter) = node.iterations.last() {
            node.iter = max_iter.iter;
        }
    }

    // Derive run-level awaiting_user from node states. Two causes, one status
    // (ADR-0049): an *interactive* node genuinely waiting on its user, OR an
    // *interrupted* node whose infra incident parked the run. Both lift a
    // `Running` run to `AwaitingUser`; only the incident carries an
    // `awaiting_reason`, which keeps the two distinguishable from the run state
    // alone.
    if state.status == RunStatus::Running
        && state
            .nodes
            .values()
            .any(|n| n.status == NodeStatus::AwaitingUser || n.status == NodeStatus::Interrupted)
    {
        state.status = RunStatus::AwaitingUser;
    }

    // Surface the incident reason when the park is caused by an interrupted node
    // and no run-level `RunInterrupted` already set one. An interactive-only
    // wait leaves `awaiting_reason` `None` (its prompt is the node's business,
    // not an incident). Deterministic pick — the lowest node id — so replay is
    // stable when several nodes interrupted.
    if state.status == RunStatus::AwaitingUser && state.awaiting_reason.is_none() {
        if let Some((_, node)) = state
            .nodes
            .iter()
            .filter(|(_, n)| n.status == NodeStatus::Interrupted)
            .min_by(|a, b| a.0.cmp(b.0))
        {
            state.awaiting_reason = node.failure_reason.clone();
            // #601: carry the machine slug too, derived from the node reason's
            // `<code>: <prose>` prefix (`session_died`, `spawn_aborted`,
            // `boot_recovery`, …). A node-level interrupt has no run-level
            // `reason_code` payload to read, so the prose prefix is the source.
            state.awaiting_reason_code = node.failure_reason.as_deref().and_then(parse_reason_code);
        }
    }

    // #588 / ADR-0069: the awaiting info only means something on an awaiting
    // node — a terminal or running node keeps none (a `NodeCompleted` after a
    // wait, a `Failed` after a refused completion, …).
    for node in state.nodes.values_mut() {
        if node.status != NodeStatus::AwaitingUser {
            node.awaiting = None;
        }
    }

    // #588: a **declared** wait carries its message as the run's prose reason —
    // the banner and the run list's dot tooltip read it — but NO machine slug:
    // `awaiting_reason_code` absent still means « not an incident » (the
    // manager's contract). Deterministic pick — the lowest node id — as above.
    if state.status == RunStatus::AwaitingUser
        && state.awaiting_reason.is_none()
        && state.awaiting_reason_code.is_none()
    {
        if let Some((_, node)) = state
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.status == NodeStatus::AwaitingUser
                    && n.awaiting.as_ref().is_some_and(|a| a.message.is_some())
            })
            .min_by(|a, b| a.0.cmp(b.0))
        {
            state.awaiting_reason = node.awaiting.as_ref().and_then(|a| a.message.clone());
        }
    }

    // ADR-0086: a `Running` Run past `End` says how many started nodes it
    // still waits for.
    state.waiting_for_started_nodes = crate::domain::on_demand::started_nodes_waited(state);
}

/// Is this Run *stalled* (#180)? A run with no node currently `running` or
/// `waiting` and no active merge resolver, where at least one node has gone
/// `stale`, has nothing left to drive it forward.
///
/// **VESTIGIAL since #469 (ADR-0032): reserved for historical Runs.** This
/// predicate tests `NodeStatus::Stale` and nothing else, and nothing in the
/// daemon produces that status any more — so it is constantly `false` for every
/// new Run, and **#180's amber dot will never light again**. That is a deliberate
/// removal of functionality, recorded in ADR-0032 § "Ce qu'on ne fait pas": do
/// not wire a producer back in to "fix" the amber dot. What replaced the stale
/// verdict is the absence of one — an agent alive but not progressing stays
/// `Running`, with its session attachable and the human's Stop/Retry to hand.
///
/// The function is kept, live and correct, because the event log is append-only:
/// a Run that recorded a `NodeStale` before #469 still projects `Stale` and must
/// still render as it always did.
pub fn is_stalled(run: &RunState) -> bool {
    if run.status != RunStatus::Running {
        return false;
    }

    let has_active_node = run
        .nodes
        .values()
        .any(|n| matches!(n.status, NodeStatus::Running | NodeStatus::Waiting));
    if has_active_node {
        return false;
    }

    run.nodes.values().any(|n| n.status == NodeStatus::Stale)
}
