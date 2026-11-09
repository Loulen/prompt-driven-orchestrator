//! The check of each node lifecycle event: a completion, a start, a failure,
//! an interrupt, a stale mark.

use super::*;

pub(super) fn validate_completion(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject(RejectReason::MissingNodeId {
            kind: event.kind.clone(),
        });
    };
    let iter = event.iter.unwrap_or(1);

    if !run_accepts_lifecycle(&state.status) {
        return Verdict::reject(RejectReason::RunNotLive {
            run_id: state.run_id.clone(),
            status: state.status.clone(),
            node_id: node_id.to_string(),
            iter,
            kind: event.kind.clone(),
        });
    }

    if let Some(reason) = duplicate_completion(state, node_id, iter) {
        return Verdict::noop(reason);
    }
    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Running) | Some(NodeStatus::AwaitingUser) | Some(NodeStatus::Failed) => {
            Verdict::Allow
        }
        Some(other) => Verdict::reject(RejectReason::IterationNotCompletable {
            node_id: node_id.to_string(),
            iter,
            status: other,
        }),
        None => {
            // A skip legitimately completes a node that never started — it marks
            // a node stuck on an unreachable input satisfied so the run does not
            // hang. Any other never-started completion stays a reject.
            if is_skip_completion(event) {
                Verdict::Allow
            } else {
                Verdict::reject(RejectReason::IterationNeverStarted {
                    node_id: node_id.to_string(),
                    iter,
                })
            }
        }
    }
}

/// True for the `skipped: true` marker of a local skip or a reachability
/// auto-skip. Such a completion is allowed even for a node that never started.
fn is_skip_completion(event: &Event) -> bool {
    event
        .payload
        .as_ref()
        .and_then(|p| p.get("skipped"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub(super) fn validate_start(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject(RejectReason::MissingNodeId {
            kind: event.kind.clone(),
        });
    };
    let iter = event.iter.unwrap_or(1);

    if !run_accepts_lifecycle(&state.status) {
        return Verdict::reject(RejectReason::RunNotLive {
            run_id: state.run_id.clone(),
            status: state.status.clone(),
            node_id: node_id.to_string(),
            iter,
            kind: event.kind.clone(),
        });
    }

    if let Some(live_iter) = live_iteration(state, node_id) {
        // Item laps of an open collection region run in parallel — a live sibling
        // lap is the intended shape, not a concurrency violation.
        if live_iter != iter && !is_open_collection_lap(state, node_id, iter) {
            return Verdict::reject(RejectReason::ConcurrentIterationLive {
                node_id: node_id.to_string(),
                live_iter,
                iter,
            });
        }
        // Same iter: legal restart/promotion of the live iteration.
    }

    // A `Skipped` iter is settled like a `Completed` one — never re-spawn it.
    if matches!(
        iteration_status(state, node_id, iter),
        Some(NodeStatus::Completed) | Some(NodeStatus::Skipped)
    ) {
        return Verdict::reject(RejectReason::IterationAlreadyCompleted {
            node_id: node_id.to_string(),
            iter,
        });
    }

    Verdict::Allow
}

/// Validate a `NodeFailed` emitted by the liveness sweep or boot recovery.
///
/// These detectors snapshot the run, decide, then emit, so the iteration may have
/// reached a terminal state organically in between. Drop the late failure as a
/// no-op rather than overwriting a completed/failed/stopped/stale iteration.
pub(super) fn validate_fail(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject(RejectReason::MissingNodeId {
            kind: event.kind.clone(),
        });
    };
    let iter = event.iter.unwrap_or(1);

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed)
        | Some(NodeStatus::Skipped)
        | Some(NodeStatus::Failed)
        | Some(NodeStatus::Stopped)
        | Some(NodeStatus::Stale) => Verdict::noop(format!(
            "node {node_id} iter {iter} is already terminal: failure ignored"
        )),
        None => Verdict::noop(format!(
            "node {node_id} iter {iter} has no started iteration: failure ignored"
        )),
        _ => Verdict::Allow,
    }
}

/// Validate a `NodeInterrupted` — an infra incident on a node (ADR-0049),
/// emitted by the liveness sweep, boot recovery, or a spawn-abort.
///
/// Two deliberate divergences from [`validate_fail`]; don't align them:
///
/// 1. A **never-started** iteration is **allowed**, not a no-op. A spawn that
///    aborts *before* `NodeStarted` (ADR-0050 §1) still names its node, and the
///    projection must materialise it `Interrupted` so the run parks visibly.
/// 2. The run-liveness gate is **not** applied. An interrupt arriving just after
///    a terminal event must be dropped as a duplicate rather than resurrect the
///    run — the already-terminal iteration arm below covers that.
pub(super) fn validate_interrupt(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject(RejectReason::MissingNodeId {
            kind: event.kind.clone(),
        });
    };
    let iter = event.iter.unwrap_or(1);

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed)
        | Some(NodeStatus::Skipped)
        | Some(NodeStatus::Failed)
        | Some(NodeStatus::Stopped)
        | Some(NodeStatus::Stale)
        | Some(NodeStatus::Interrupted) => Verdict::noop(format!(
            "node {node_id} iter {iter} is already terminal: interrupt ignored"
        )),
        // Never started: a fresh interrupt is allowed unless the node itself is
        // already interrupted (a re-interrupt is a no-op).
        None if state
            .nodes
            .get(node_id)
            .is_some_and(|n| n.status == NodeStatus::Interrupted) =>
        {
            Verdict::noop(format!(
                "node {node_id} is already interrupted: interrupt ignored"
            ))
        }
        _ => Verdict::Allow,
    }
}

pub(super) fn validate_stale(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject(RejectReason::MissingNodeId {
            kind: event.kind.clone(),
        });
    };
    let iter = event.iter.unwrap_or(1);

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed)
        | Some(NodeStatus::Skipped)
        | Some(NodeStatus::Failed)
        | Some(NodeStatus::Stopped)
        | Some(NodeStatus::Stale) => Verdict::noop(format!(
            "node {node_id} iter {iter} is already terminal: stale marker ignored"
        )),
        None => Verdict::noop(format!(
            "node {node_id} iter {iter} has no started iteration: stale marker ignored"
        )),
        _ => Verdict::Allow,
    }
}
