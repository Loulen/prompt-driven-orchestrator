//! The two halves of a node's completion: the pure head
//! [`evaluate_completion_head`] (which completion is legal) and the
//! side-effecting tail [`complete_node`] (record it, fire the node's edges).
//! Don't add side effects to the head: its purity is what lets the three
//! completion callers share it.

use crate::domain::{event, projection, state};
use tracing::error;

use crate::application::events::load_events;
use crate::application::scheduler::node_completion::handle_node_completion;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::domain::transition_guard;
use crate::Runs;

use super::advance::*;

/// Which order the completion tail runs the producer's edge-firing pass
/// ([`handle_node_completion`]) relative to the readiness sweep ([`advance_run`]).
///
/// The two orders are believed behavior-equivalent (the two passes cover disjoint
/// spawn sets and `spawn_node` re-validates every transition), but don't collapse
/// them to one variant without an order-equivalence integration test first —
/// nothing here would catch a divergence.
pub enum CompletionOrder {
    /// `node_done` & `handle_merge_resolver_done`: edges, then sweep.
    CompletionFirst,
    /// `mark_node_done` arm: sweep, then edges (the interactive node is already gone).
    SweepFirst,
}

/// What the completion tail did — lets each caller keep its own log line / HTTP
/// response while sharing the tail.
pub enum CompletionOutcome {
    /// `RunCompleted` was emitted on this call.
    RunCompleted,
    /// The run advanced but not all expected nodes are done yet — or it completed
    /// earlier in the same tail via an HNC `Complete`/`Halt` action. Either way
    /// the completion gate emitted nothing.
    StillRunning,
    Halted,
}

/// The three-way outcome of the pure completion **head** decision.
///
/// The caller's contract per variant:
/// - [`CompletionHead::Reject`] → `409 CONFLICT { error }`.
/// - [`CompletionHead::NoOp`]   → `200 { ok, noop, reason }`.
/// - [`CompletionHead::Allow`]  → run your own side effects, append your own
///   `NodeCompleted`, drive your own tail.
pub enum CompletionHead {
    Reject { reason: String },
    NoOp { reason: String },
    Allow,
}

/// The shared, **pure** *head* of node completion, used by `node_done`, the
/// `mark_node_done` command arm and `node_skip`.
///
/// Decides only *whether* the completion is legal: no DB, no tmux, no append, no
/// clock (the guard is ts-blind, so the probe carries an empty `ts`). Keep it
/// that way — see the module header.
///
/// `None` `run_state` yields `Allow`; each caller decides whether to admit it
/// (`node_done` and `node_skip` 404 *before* calling; `mark` forwards it).
pub fn evaluate_completion_head(
    run_state: Option<&state::RunState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> CompletionHead {
    let probe = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: String::new(),
        kind: event::EventKind::NodeCompleted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: None,
    };
    match transition_guard::validate_transition(run_state, &probe) {
        transition_guard::Verdict::Reject { reason } => CompletionHead::Reject {
            reason: reason.to_string(),
        },
        transition_guard::Verdict::NoOp { reason } => CompletionHead::NoOp { reason },
        transition_guard::Verdict::Allow => CompletionHead::Allow,
    }
}

/// The completion head for a **local skip**: identical to
/// [`evaluate_completion_head`] but the probe carries `skipped: true`, so a node
/// that never started is `Allow`ed — a skip legitimately satisfies a node stuck
/// waiting on an unreachable input.
pub fn evaluate_skip_completion_head(
    run_state: Option<&state::RunState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> CompletionHead {
    let probe = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: String::new(),
        kind: event::EventKind::NodeCompleted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "skipped": true })),
    };
    match transition_guard::validate_transition(run_state, &probe) {
        transition_guard::Verdict::Reject { reason } => CompletionHead::Reject {
            reason: reason.to_string(),
        },
        transition_guard::Verdict::NoOp { reason } => CompletionHead::NoOp { reason },
        transition_guard::Verdict::Allow => CompletionHead::Allow,
    }
}

/// Reload + re-project, then fire the just-completed producer's outgoing edges.
///
/// Don't pass the caller's projection through: `handle_node_completion` does not
/// re-project on its first pass, so it needs a fresh `events` slice.
pub(super) async fn fire_edges(state: &Runs, run_id: &str, completed_node_id: &str) {
    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("complete_node: failed to load events for {run_id}: {e}");
            return;
        }
    };
    let Some(run_state) = projection::project(&events) else {
        return;
    };
    handle_node_completion(state, &run_state, run_id, completed_node_id, &events).await;
}

/// The shared post-`NodeCompleted` tail that drives a Run forward after one of
/// its nodes completes (ADR-0009).
///
/// PRECONDITION: the caller has already appended its `NodeCompleted` (plus any
/// companion events) and done any session reap. `completed_node_id` is the node
/// whose edges to fire — on the merge-resolver path that is the *original
/// conflicting node*, not the route's `__merge_resolver__` param, so it cannot be
/// re-derived from the request.
///
/// `retry_waiting_nodes` is cross-run on purpose: a freed session slot can start a
/// `waiting` node in another run. Never call this tail from an all-runs/waiting
/// sweep — see [`maybe_complete_run`]'s single-emitter rule.
pub async fn complete_node(
    state: &Runs,
    run_id: &str,
    completed_node_id: &str,
    order: CompletionOrder,
    complete_when_awaiting_user: bool,
) -> CompletionOutcome {
    match order {
        CompletionOrder::CompletionFirst => {
            fire_edges(state, run_id, completed_node_id).await;
            advance_run(state, run_id).await;
            retry_waiting_nodes(state).await;
        }
        CompletionOrder::SweepFirst => {
            advance_run(state, run_id).await;
            retry_waiting_nodes(state).await;
            fire_edges(state, run_id, completed_node_id).await;
        }
    }

    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("complete_node: failed to reload events for {run_id}: {e}");
            return CompletionOutcome::StillRunning;
        }
    };
    let Some(run_state) = projection::project(&events) else {
        return CompletionOutcome::StillRunning;
    };
    if run_state.status == state::RunStatus::Halted {
        return CompletionOutcome::Halted;
    }
    let expected = expected_completion_node_ids(&run_state);
    if maybe_complete_run(
        state,
        run_id,
        &expected,
        &run_state,
        complete_when_awaiting_user,
    )
    .await
    {
        CompletionOutcome::RunCompleted
    } else {
        CompletionOutcome::StillRunning
    }
}
