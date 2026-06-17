//! Transition guard for the event-log projection (#212).
//!
//! Single chokepoint answering one question: *is this lifecycle event legal
//! given the currently projected run state?* Every emitter of node-lifecycle
//! events (`node_done`, `mark_node_done`, `resume_run` re-evaluation,
//! `restart_node`, the stale detector, the scheduler spawn paths) must consult
//! this guard **before** appending — an illegal transition is rejected before
//! the append, never compensated after.
//!
//! Pure module: no IO, no clock, no DB. Testable in isolation against a
//! projected [`RunState`].

use crate::event_log::{Event, EventKind, NodeStatus, RunState, RunStatus};

/// Outcome of validating a lifecycle event against the projected state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The transition is legal: append it.
    Allow,
    /// The transition is a legal duplicate (e.g. a second completion of an
    /// already-completed iteration): skip the append AND any downstream
    /// re-evaluation, but do not surface an error.
    NoOp { reason: String },
    /// The transition is illegal: refuse the append and surface the reason to
    /// the caller (fail-fast, never silent).
    Reject { reason: String },
}

impl Verdict {
    fn noop(reason: impl Into<String>) -> Self {
        Verdict::NoOp {
            reason: reason.into(),
        }
    }

    fn reject(reason: impl Into<String>) -> Self {
        Verdict::Reject {
            reason: reason.into(),
        }
    }
}

fn run_accepts_lifecycle(status: &RunStatus) -> bool {
    matches!(status, RunStatus::Running | RunStatus::AwaitingUser)
}

fn iteration_status(state: &RunState, node_id: &str, iter: i64) -> Option<NodeStatus> {
    state
        .nodes
        .get(node_id)
        .and_then(|n| n.iterations.iter().find(|i| i.iter == iter))
        .map(|i| i.status.clone())
}

/// The iteration currently holding (or owed) a live agent session for this
/// node, if any: `Running` or `AwaitingUser` iteration rows, or the node-level
/// `Waiting` marker (throttled, no iteration row yet — #159).
fn live_iteration(state: &RunState, node_id: &str) -> Option<i64> {
    let node = state.nodes.get(node_id)?;
    if node.status == NodeStatus::Waiting {
        return Some(node.iter);
    }
    node.iterations
        .iter()
        .filter(|i| matches!(i.status, NodeStatus::Running | NodeStatus::AwaitingUser))
        .map(|i| i.iter)
        .max()
}

/// Validate a lifecycle event against the projected run state.
///
/// Non-lifecycle kinds are always allowed: the guard governs node lifecycle
/// transitions (`NodeStarted`, `NodeWaiting`, `NodeCompleted`,
/// `NodeAutoCompleted`, `NodeStale`), not control-flow bookkeeping.
pub fn validate_transition(state: Option<&RunState>, event: &Event) -> Verdict {
    let Some(state) = state else {
        // No projected state yet (run not started): nothing to validate
        // against. The first RunStarted event creates the state.
        return Verdict::Allow;
    };

    match event.kind {
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
            validate_completion(state, event)
        }
        EventKind::NodeStarted | EventKind::NodeWaiting => validate_start(state, event),
        EventKind::NodeStale => validate_stale(state, event),
        EventKind::NodeFailed => validate_fail(state, event),
        _ => Verdict::Allow,
    }
}

/// Scheduler-side dedup for proposed `Spawn { node, iter }` actions on
/// re-evaluation paths (resume_run, extend_cycle, region routes, loop/foreach
/// body completion). A proposal is superfluous when the node already has a
/// live iteration — any iter, *including* the proposed one: a running session
/// must never be doubled by the scheduler (restart_node alone may re-spawn a
/// live iteration) — or when the proposed iteration has already completed.
///
/// Returns the human-readable reason when the proposal should be skipped.
pub fn spawn_superfluous(state: &RunState, node_id: &str, iter: i64) -> Option<String> {
    if let Some(live_iter) = live_iteration(state, node_id) {
        return Some(format!(
            "node {node_id} iter {live_iter} is live: scheduler will not spawn iter {iter}"
        ));
    }
    if iteration_status(state, node_id, iter) == Some(NodeStatus::Completed) {
        return Some(format!(
            "node {node_id} iter {iter} already completed: nothing to spawn"
        ));
    }
    None
}

fn validate_completion(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject("completion event without node_id");
    };
    let iter = event.iter.unwrap_or(1);

    if !run_accepts_lifecycle(&state.status) {
        return Verdict::reject(format!(
            "run {} is {:?}: cannot complete node {node_id} iter {iter} — resume the run first",
            state.run_id, state.status
        ));
    }

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed) => Verdict::noop(format!(
            "node {node_id} iter {iter} is already completed: duplicate completion ignored"
        )),
        Some(NodeStatus::Running) | Some(NodeStatus::AwaitingUser) | Some(NodeStatus::Failed) => {
            Verdict::Allow
        }
        Some(other) => Verdict::reject(format!(
            "node {node_id} iter {iter} is {other:?}: cannot complete"
        )),
        None => Verdict::reject(format!(
            "node {node_id} iter {iter} was never started (no node_started event): cannot complete"
        )),
    }
}

fn validate_start(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject("start event without node_id");
    };
    let iter = event.iter.unwrap_or(1);

    if !run_accepts_lifecycle(&state.status) {
        return Verdict::reject(format!(
            "run {} is {:?}: no scheduling on a non-running run — resume the run first",
            state.run_id, state.status
        ));
    }

    if let Some(live_iter) = live_iteration(state, node_id) {
        if live_iter != iter {
            return Verdict::reject(format!(
                "node {node_id} iter {live_iter} is still live: refusing concurrent iter {iter}"
            ));
        }
        // Same iter: legal restart/promotion of the live iteration.
    }

    if iteration_status(state, node_id, iter) == Some(NodeStatus::Completed) {
        return Verdict::reject(format!(
            "node {node_id} iter {iter} already completed: refusing to re-run it"
        ));
    }

    Verdict::Allow
}

/// Validate a `NodeFailed` emitted by the liveness sweep or boot recovery
/// (#213). These detectors snapshot the run, decide, then emit — by the time
/// the failure lands the iteration may already have reached a terminal state
/// organically. The guard drops the late failure as a no-op rather than
/// overwriting a completed/failed/stopped/stale iteration, and ignores a
/// failure for an iteration that was never started (nothing live to fail).
/// A failure on a live (`Running`/`AwaitingUser`/`Waiting`) iteration — the
/// reason the detectors exist — is allowed.
fn validate_fail(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject("fail event without node_id");
    };
    let iter = event.iter.unwrap_or(1);

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed)
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

fn validate_stale(state: &RunState, event: &Event) -> Verdict {
    let Some(node_id) = event.node_id.as_deref() else {
        return Verdict::reject("stale event without node_id");
    };
    let iter = event.iter.unwrap_or(1);

    match iteration_status(state, node_id, iter) {
        Some(NodeStatus::Completed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{now_iso, project};

    fn ev(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
        let payload = if kind == EventKind::RunStarted {
            Some(serde_json::json!({ "pipeline_name": "test" }))
        } else {
            None
        };
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: now_iso(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload,
        }
    }

    fn state_from(events: &[Event]) -> RunState {
        project(events).expect("projected state")
    }

    fn assert_reject(verdict: Verdict, expected_fragment: &str) {
        match verdict {
            Verdict::Reject { reason } => assert!(
                reason.contains(expected_fragment),
                "reject reason {reason:?} should mention {expected_fragment:?}"
            ),
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    fn assert_noop(verdict: Verdict) {
        assert!(
            matches!(verdict, Verdict::NoOp { .. }),
            "expected NoOp, got {verdict:?}"
        );
    }

    // --- NodeCompleted ---

    #[test]
    fn duplicate_completion_of_completed_iteration_is_noop() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        );
        assert_noop(verdict);
    }

    #[test]
    fn completion_of_running_iteration_is_allowed() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn completion_of_never_started_iteration_is_rejected() {
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("ghost"), Some(1)),
        );
        assert_reject(verdict, "never started");
    }

    #[test]
    fn completion_of_unstarted_higher_iteration_is_rejected() {
        // worker ran iter 1 only; completing iter 2 (never started) is illegal.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(2)),
        );
        assert_reject(verdict, "never started");
    }

    #[test]
    fn completion_on_failed_run_is_rejected() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::RunFailed, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        );
        assert_reject(verdict, "resume the run");
    }

    #[test]
    fn completion_of_failed_iteration_is_allowed() {
        // mark_node_done on a failed node (outputs fixed by hand) is a
        // supported recovery path.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeFailed, Some("worker"), Some(1)),
            // node_fail also fails the run; a resume_run lifts it.
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn auto_completion_follows_completion_rules() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeAutoCompleted, Some("worker"), Some(1)),
        );
        assert_noop(verdict);
    }

    // --- NodeStarted ---

    #[test]
    fn start_while_another_iteration_is_live_is_rejected() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(3)),
        );
        assert_reject(verdict, "still live");
    }

    #[test]
    fn restart_of_the_live_iteration_is_allowed() {
        // restart_node kills the session and re-spawns the SAME iter.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn restart_of_older_iter_while_newer_is_live_is_rejected() {
        // #196: restart_node iter 1 raced the scheduler's iter-2 spawn.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("griller"), Some(1)),
            ev(EventKind::NodeCompleted, Some("griller"), Some(1)),
            ev(EventKind::NodeStarted, Some("griller"), Some(2)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("griller"), Some(1)),
        );
        assert_reject(verdict, "still live");
    }

    #[test]
    fn start_of_already_completed_iteration_is_rejected() {
        // #195/#198: never redo completed work.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_reject(verdict, "already completed");
    }

    #[test]
    fn start_on_failed_run_is_rejected() {
        // #197: a failed run schedules nothing until resume_run.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::RunFailed, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_reject(verdict, "non-running run");
    }

    #[test]
    fn start_of_fresh_node_is_allowed() {
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn start_of_next_iter_after_completed_iter_is_allowed() {
        // Lap advance: iter 1 completed, nothing live, iter 2 may start.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn promotion_of_waiting_node_at_same_iter_is_allowed() {
        // #159 throttling: NodeWaiting then NodeStarted at the same iter.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeWaiting, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn start_of_other_iter_while_node_is_waiting_is_rejected() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeWaiting, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        );
        assert_reject(verdict, "still live");
    }

    #[test]
    fn restart_of_failed_iteration_is_allowed() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeFailed, Some("worker"), Some(1)),
            // resume_run lifted the run-level failure.
            ev(EventKind::RunResumed, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    // --- NodeStale ---

    #[test]
    fn stale_on_completed_iteration_is_noop() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStale, Some("worker"), Some(1)),
        );
        assert_noop(verdict);
    }

    #[test]
    fn stale_on_running_iteration_is_allowed() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStale, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    // --- NodeFailed (detection / recovery, #213) ---

    #[test]
    fn fail_on_completed_iteration_is_noop() {
        // The liveness sweep / boot recovery snapshots the run, then emits a
        // failure. If the node completed organically in between, the guard must
        // drop the late failure as a no-op — never overwrite a completed node.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeFailed, Some("worker"), Some(1)),
        );
        assert_noop(verdict);
    }

    #[test]
    fn fail_on_running_iteration_is_allowed() {
        // The liveness sweep marking a Running node Failed (dead session) is
        // exactly the transition #213 exists to allow.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeFailed, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn fail_on_never_started_iteration_is_noop() {
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeFailed, Some("ghost"), Some(1)),
        );
        assert_noop(verdict);
    }

    // --- spawn_superfluous (scheduler-side dedup) ---

    #[test]
    fn spawn_proposal_for_running_node_is_superfluous_at_any_iter() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("b"), Some(1)),
        ]);
        // Same iter: the running session must not be doubled by the scheduler.
        assert!(spawn_superfluous(&state, "b", 1).is_some());
        // Next iter: #201 — no concurrent second iteration.
        assert!(spawn_superfluous(&state, "b", 2).is_some());
    }

    #[test]
    fn spawn_proposal_for_completed_iteration_is_superfluous() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("b"), Some(1)),
            ev(EventKind::NodeCompleted, Some("b"), Some(1)),
        ]);
        assert!(spawn_superfluous(&state, "b", 1).is_some());
        // A fresh lap at iter 2 is legitimate (extend_cycle / region laps).
        assert!(spawn_superfluous(&state, "b", 2).is_none());
    }

    #[test]
    fn spawn_proposal_for_fresh_or_failed_node_is_not_superfluous() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("f"), Some(1)),
            ev(EventKind::NodeFailed, Some("f"), Some(1)),
            ev(EventKind::RunResumed, None, None),
        ]);
        assert!(spawn_superfluous(&state, "never-ran", 1).is_none());
        assert!(spawn_superfluous(&state, "f", 1).is_none());
    }

    #[test]
    fn spawn_proposal_for_waiting_node_is_superfluous() {
        // A throttled node is owned by retry_waiting_nodes; re-evaluation
        // paths must not double-schedule it.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeWaiting, Some("w"), Some(1)),
        ]);
        assert!(spawn_superfluous(&state, "w", 1).is_some());
    }

    // --- non-lifecycle kinds and missing state ---

    #[test]
    fn non_lifecycle_kinds_are_always_allowed() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::RunFailed, None, None),
        ]);
        for kind in [
            EventKind::CommandIssued,
            EventKind::RunResumed,
            EventKind::NodeStopped,
            EventKind::RunCompleted,
        ] {
            let verdict = validate_transition(Some(&state), &ev(kind, Some("worker"), Some(1)));
            assert_eq!(verdict, Verdict::Allow);
        }
    }

    #[test]
    fn missing_state_is_allowed() {
        let verdict =
            validate_transition(None, &ev(EventKind::NodeStarted, Some("worker"), Some(1)));
        assert_eq!(verdict, Verdict::Allow);
    }
}
