//! Transition guard for the event-log projection: *is this lifecycle event legal
//! given the currently projected run state?*
//!
//! Every emitter of node-lifecycle events must consult the guard **before**
//! appending. Don't compensate after the append — the projection has no undo.
//!
//! Keep the module pure: no IO, no clock, no DB.

use crate::event_log::{Event, EventKind, NodeStatus, RunState, RunStatus};

/// Outcome of validating a lifecycle event against the projected state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The transition is legal: append it.
    Allow,
    /// The transition is a legal duplicate (e.g. a second completion of an
    /// already-completed iteration): skip the append AND any downstream
    /// re-evaluation, but do not surface an error.
    NoOp { reason: String },
    /// The transition is illegal: refuse the append and surface the reason to
    /// the caller (fail-fast, never silent).
    Reject { reason: RejectReason },
}

impl Verdict {
    fn noop(reason: impl Into<String>) -> Self {
        Verdict::NoOp {
            reason: reason.into(),
        }
    }

    fn reject(reason: RejectReason) -> Self {
        Verdict::Reject { reason }
    }
}

/// Why the transition guard refused a lifecycle event — **one variant per tested
/// condition**, never per narration. Add a variant only when the *predicate*
/// differs; two refusals that test the same thing share one, with `kind`
/// selecting the prose template.
///
/// `Display` is wire-visible: it reproduces the exact `message` clients already
/// receive, so don't reword it casually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RejectReason {
    /// Lifecycle event with no `node_id`.
    MissingNodeId { kind: EventKind },
    /// The Run does not accept lifecycle events (not `Running`/`AwaitingUser`).
    /// Covers both completion and start; `node_id`/`iter` feed the completion
    /// template only, and `kind` picks the template.
    RunNotLive {
        run_id: String,
        status: RunStatus,
        node_id: String,
        iter: i64,
        kind: EventKind,
    },
    /// Start refused: a **different** iteration holds the live slot, outside an
    /// open collection lap. Don't rename this `newer_iteration_live` (the
    /// ADR-0037 trap): the guard tests `live_iter != iter`, so `live_iter` may
    /// be **older** than `iter`.
    ConcurrentIterationLive {
        node_id: String,
        live_iter: i64,
        iter: i64,
    },
    /// Start refused: the iteration is already `Completed` (never redo it).
    IterationAlreadyCompleted { node_id: String, iter: i64 },
    /// Completion refused: the iteration exists but sits in a status one cannot
    /// complete from (`Stopped`/`Stale`/…).
    IterationNotCompletable {
        node_id: String,
        iter: i64,
        status: NodeStatus,
    },
    /// Completion refused: no iteration ever started.
    IterationNeverStarted { node_id: String, iter: i64 },
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::MissingNodeId { kind } => {
                let verb = match kind {
                    EventKind::NodeCompleted | EventKind::NodeAutoCompleted => "completion",
                    EventKind::NodeStarted | EventKind::NodeWaiting => "start",
                    EventKind::NodeFailed => "fail",
                    EventKind::NodeInterrupted => "interrupt",
                    EventKind::NodeStale => "stale",
                    _ => "lifecycle",
                };
                write!(f, "{verb} event without node_id")
            }
            RejectReason::RunNotLive {
                run_id,
                status,
                node_id,
                iter,
                kind,
            } => match kind {
                EventKind::NodeCompleted | EventKind::NodeAutoCompleted => write!(
                    f,
                    "run {run_id} is {status:?}: cannot complete node {node_id} iter {iter} — resume the run first"
                ),
                _ => write!(
                    f,
                    "run {run_id} is {status:?}: no scheduling on a non-running run — resume the run first"
                ),
            },
            RejectReason::ConcurrentIterationLive {
                node_id,
                live_iter,
                iter,
            } => write!(
                f,
                "node {node_id} iter {live_iter} is still live: refusing concurrent iter {iter}"
            ),
            RejectReason::IterationAlreadyCompleted { node_id, iter } => write!(
                f,
                "node {node_id} iter {iter} already completed: refusing to re-run it"
            ),
            RejectReason::IterationNotCompletable {
                node_id,
                iter,
                status,
            } => write!(f, "node {node_id} iter {iter} is {status:?}: cannot complete"),
            RejectReason::IterationNeverStarted { node_id, iter } => write!(
                f,
                "node {node_id} iter {iter} was never started (no node_started event): cannot complete"
            ),
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
pub(crate) fn validate_transition(state: Option<&RunState>, event: &Event) -> Verdict {
    let Some(state) = state else {
        return Verdict::Allow;
    };

    match event.kind {
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
            validate_completion(state, event)
        }
        EventKind::NodeStarted | EventKind::NodeWaiting => validate_start(state, event),
        EventKind::NodeStale => validate_stale(state, event),
        EventKind::NodeFailed => validate_fail(state, event),
        EventKind::NodeInterrupted => validate_interrupt(state, event),
        _ => Verdict::Allow,
    }
}

/// Is `(node_id, iter)` a **parallel item lap** of a collection region whose
/// barrier is still open?
///
/// The single, narrow exemption to "a node has at most one live iteration": a
/// `kind: collection` region is a *parallel* fan-out of the same node
/// (ADR-0011 / ADR-0026), the exact shape the guard stops everywhere else.
/// Without the exemption, laps >= 2 are refused 1 ms after lap 1 starts, the
/// barrier never fires, and the Run wedges `running` with no live session.
///
/// Don't widen the scope — the chokepoint must keep holding for `restart_node`,
/// the liveness sweep and boot recovery:
/// - the region must exist in the projection and still be **open** (`!done`) —
///   once the barrier fires the node is a plain node again;
/// - `node_id` must be one the region **governs** (its entry or a member);
/// - `iter` must be a real lap index, `1..=total_items` — an out-of-range
///   iteration is not an item and stays refused.
///
/// Says nothing about *completed* laps: re-running a finished lap is refused by
/// the caller as it always was.
fn is_open_collection_lap(state: &RunState, node_id: &str, iter: i64) -> bool {
    state
        .collection_states
        .values()
        .any(|cs| !cs.done && iter >= 1 && iter <= cs.total_items && cs.governs(node_id))
}

/// Scheduler-side dedup for proposed `Spawn { node, iter }` actions on
/// re-evaluation paths (resume_run, extend_cycle, region routes, loop/foreach
/// body completion). A proposal is superfluous when the node already has a
/// live iteration — any iter, *including* the proposed one: a running session
/// must never be doubled by the scheduler (restart_node alone may re-spawn a
/// live iteration) — or when the proposed iteration has already completed.
///
/// Returns the human-readable reason when the proposal should be skipped.
pub(crate) fn spawn_superfluous(state: &RunState, node_id: &str, iter: i64) -> Option<String> {
    if let Some(live_iter) = live_iteration(state, node_id) {
        // A sibling item lap of an open collection region is concurrent work by
        // design; only a proposal for the live lap ITSELF is superfluous.
        if !(live_iter != iter && is_open_collection_lap(state, node_id, iter)) {
            return Some(format!(
                "node {node_id} iter {live_iter} is live: scheduler will not spawn iter {iter}"
            ));
        }
    }
    // A `Skipped` iteration is as settled as a `Completed` one — never re-spawn.
    if matches!(
        iteration_status(state, node_id, iter),
        Some(NodeStatus::Completed) | Some(NodeStatus::Skipped)
    ) {
        return Some(format!(
            "node {node_id} iter {iter} already completed: nothing to spawn"
        ));
    }
    None
}

/// Head-of-handler precondition for a **retry** (`node_retry`).
///
/// `node_retry` must call this as its FIRST gesture — before it stops the node and
/// before its two `invalidate_nodes`. A refusal landing any later leaves the
/// self-invalidation committed (`NodeInvalidated` is not a lifecycle transition,
/// so the guard never refuses it) and the node is gone from the projection with no
/// replacement: the "`pending` forever" freeze of #496.
///
/// Don't swap the predicate for a synthetic `NodeStarted` probe: retry
/// legitimately re-spawns a `Running` node (it stops it first) and a `Completed`
/// node (it advances to `iter+1`), both of which [`validate_start`] refuses. Ask
/// only what a retry shares with every spawn — is the Run live? — and let the
/// handler resolve concurrency itself.
///
/// `iter` feeds the refusal template only; the predicate ignores it.
pub(crate) fn retry_run_precondition(
    state: &RunState,
    node_id: &str,
    iter: i64,
) -> Option<RejectReason> {
    if run_accepts_lifecycle(&state.status) {
        return None;
    }
    Some(RejectReason::RunNotLive {
        run_id: state.run_id.clone(),
        status: state.status.clone(),
        node_id: node_id.to_string(),
        iter,
        kind: EventKind::NodeStarted,
    })
}

fn validate_completion(state: &RunState, event: &Event) -> Verdict {
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

    match iteration_status(state, node_id, iter) {
        // A `Skipped` iter is a settled completion too: a duplicate completion
        // landing on it is a no-op, not a reject.
        Some(NodeStatus::Completed) | Some(NodeStatus::Skipped) => Verdict::noop(format!(
            "node {node_id} iter {iter} is already completed: duplicate completion ignored"
        )),
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

fn validate_start(state: &RunState, event: &Event) -> Verdict {
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
fn validate_fail(state: &RunState, event: &Event) -> Verdict {
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
fn validate_interrupt(state: &RunState, event: &Event) -> Verdict {
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

fn validate_stale(state: &RunState, event: &Event) -> Verdict {
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
            Verdict::Reject { reason } => {
                let rendered = reason.to_string();
                assert!(
                    rendered.contains(expected_fragment),
                    "reject reason {rendered:?} should mention {expected_fragment:?}"
                );
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    fn assert_noop(verdict: Verdict) {
        assert!(
            matches!(verdict, Verdict::NoOp { .. }),
            "expected NoOp, got {verdict:?}"
        );
    }

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
    fn skip_completion_of_never_started_iteration_is_allowed() {
        // The one exception to "a completion needs a start".
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        let mut skip = ev(EventKind::NodeCompleted, Some("orphan"), Some(1));
        skip.payload = Some(serde_json::json!({ "skipped": true }));
        assert_eq!(validate_transition(Some(&state), &skip), Verdict::Allow);
    }

    #[test]
    fn skip_completion_of_an_already_completed_iteration_still_noops() {
        let mut done = ev(EventKind::NodeCompleted, Some("orphan"), Some(1));
        done.payload = Some(serde_json::json!({ "skipped": true }));
        let state = state_from(&[ev(EventKind::RunStarted, None, None), done.clone()]);
        assert_noop(validate_transition(Some(&state), &done));
    }

    #[test]
    fn completion_of_unstarted_higher_iteration_is_rejected() {
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
        // Fixing outputs by hand then marking done is a supported recovery path.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeFailed, Some("worker"), Some(1)),
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
    fn start_on_completed_run_is_rejected() {
        // Backstop: a re-evaluation path (resume_run on a finished run) must not
        // re-spawn an already satisfied node/loop.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
            ev(EventKind::RunCompleted, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        );
        assert_reject(verdict, "non-running run");
    }

    #[test]
    fn completion_on_completed_run_is_rejected() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
            ev(EventKind::RunCompleted, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        );
        assert_reject(verdict, "resume the run");
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
            ev(EventKind::RunResumed, None, None),
        ]);
        let verdict = validate_transition(
            Some(&state),
            &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        );
        assert_eq!(verdict, Verdict::Allow);
    }

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

    #[test]
    fn fail_on_completed_iteration_is_noop() {
        // The detector snapshots, then emits; a node that completed in between
        // must not be overwritten.
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

    #[test]
    fn interrupt_on_running_iteration_is_allowed() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        assert_eq!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeInterrupted, Some("worker"), Some(1))
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn interrupt_on_never_started_iteration_is_allowed() {
        // ADR-0050 §1: unlike NodeFailed, a before-start abort is ALLOWED so the
        // projection can materialise the node and park the run visibly.
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        assert_eq!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeInterrupted, Some("ghost"), Some(1))
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn interrupt_on_completed_iteration_is_noop() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        assert_noop(validate_transition(
            Some(&state),
            &ev(EventKind::NodeInterrupted, Some("worker"), Some(1)),
        ));
    }

    #[test]
    fn spawn_proposal_for_running_node_is_superfluous_at_any_iter() {
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("b"), Some(1)),
        ]);
        assert!(spawn_superfluous(&state, "b", 1).is_some());
        // No concurrent second iteration.
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
        // A throttled node is owned by retry_waiting_nodes.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeWaiting, Some("w"), Some(1)),
        ]);
        assert!(spawn_superfluous(&state, "w", 1).is_some());
    }

    /// `collection_started` for a single-member region over `total` items.
    fn collection_started(total: i64) -> Event {
        let mut e = ev(EventKind::CollectionStarted, None, None);
        e.payload = Some(serde_json::json!({
            "region_id": "fan",
            "entry": "worker",
            "members": ["worker"],
            "total_items": total,
        }));
        e
    }

    #[test]
    fn sibling_item_laps_of_an_open_collection_region_may_run_concurrently() {
        // ADR-0011 / ADR-0026: `Spawn { entry, iter: 1..=total }` in one burst.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            collection_started(3),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        for iter in [2, 3] {
            assert_eq!(
                validate_transition(
                    Some(&state),
                    &ev(EventKind::NodeStarted, Some("worker"), Some(iter))
                ),
                Verdict::Allow,
                "lap {iter} of a 3-item region must start alongside lap 1"
            );
        }
    }

    #[test]
    fn a_completed_item_lap_is_never_re_run() {
        // The exemption covers concurrency, not resurrection.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            collection_started(2),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
            ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        ]);
        assert_reject(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ),
            "already completed",
        );
    }

    #[test]
    fn a_node_outside_the_region_keeps_the_concurrency_refusal() {
        // The narrowness the other callers depend on: an open region exempts ITS
        // members and nobody else.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            collection_started(3),
            ev(EventKind::NodeStarted, Some("outsider"), Some(1)),
        ]);
        assert_reject(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeStarted, Some("outsider"), Some(2)),
            ),
            "still live",
        );
    }

    #[test]
    fn spawn_proposal_for_a_sibling_item_lap_is_not_superfluous() {
        // Re-evaluation must be able to propose an unstarted lap even while a
        // sibling lap holds a session.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            collection_started(2),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        assert!(spawn_superfluous(&state, "worker", 2).is_none());
        // Re-proposing the LIVE lap is still superfluous.
        assert!(spawn_superfluous(&state, "worker", 1).is_some());
        assert!(spawn_superfluous(&state, "worker", 3).is_some());
    }

    #[test]
    fn retry_precondition_refuses_a_terminal_run_with_resume_prose() {
        // The #496 incident: Play on a node of a Failed run. ADR-0035 reuses this
        // prose verbatim, so the message text is load-bearing.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::RunFailed, None, None),
        ]);
        let reason =
            retry_run_precondition(&state, "worker", 2).expect("a Failed run must refuse a retry");
        assert!(matches!(reason, RejectReason::RunNotLive { .. }));
        assert!(reason.to_string().contains("resume the run first"));
    }

    #[test]
    fn retry_precondition_allows_a_running_node_on_a_live_run() {
        // What a synthetic `NodeStarted` probe would break: a still-`Running` node
        // on a live Run. The handler stops it first.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
        ]);
        assert!(retry_run_precondition(&state, "worker", 2).is_none());
    }

    #[test]
    fn retry_precondition_allows_a_completed_node_on_a_live_run() {
        // Likewise: retrying a `Completed` node advances to `iter+1`.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        assert!(retry_run_precondition(&state, "worker", 2).is_none());
    }

    #[test]
    fn retry_precondition_refuses_a_paused_run() {
        // A Paused run is refused too: no implicit resume (ADR-0009).
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::RunPaused, None, None),
        ]);
        assert!(retry_run_precondition(&state, "worker", 2).is_some());
    }

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

    #[test]
    fn each_refusal_carries_its_typed_reject_reason() {
        // Driven through the real guard: an edit that keeps the prose but swaps
        // the variant fails here, where `assert_reject` would still pass.

        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(2)),
        ]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeStarted, Some("worker"), Some(3))
            ),
            Verdict::Reject {
                reason: RejectReason::ConcurrentIterationLive {
                    live_iter: 2,
                    iter: 3,
                    ..
                }
            }
        ));

        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::RunFailed, None, None),
        ]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeStarted, Some("worker"), Some(1))
            ),
            Verdict::Reject {
                reason: RejectReason::RunNotLive {
                    kind: EventKind::NodeStarted,
                    ..
                }
            }
        ));

        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::RunFailed, None, None),
        ]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeCompleted, Some("worker"), Some(1))
            ),
            Verdict::Reject {
                reason: RejectReason::RunNotLive {
                    kind: EventKind::NodeCompleted,
                    ..
                }
            }
        ));

        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeStarted, Some("worker"), Some(1))
            ),
            Verdict::Reject {
                reason: RejectReason::IterationAlreadyCompleted { .. }
            }
        ));

        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("worker"), Some(1)),
            ev(EventKind::NodeStopped, Some("worker"), Some(1)),
        ]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeCompleted, Some("worker"), Some(1))
            ),
            Verdict::Reject {
                reason: RejectReason::IterationNotCompletable {
                    status: NodeStatus::Stopped,
                    ..
                }
            }
        ));

        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        assert!(matches!(
            validate_transition(
                Some(&state),
                &ev(EventKind::NodeCompleted, Some("ghost"), Some(1))
            ),
            Verdict::Reject {
                reason: RejectReason::IterationNeverStarted { .. }
            }
        ));

        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        assert!(matches!(
            validate_transition(Some(&state), &ev(EventKind::NodeStarted, None, Some(1))),
            Verdict::Reject {
                reason: RejectReason::MissingNodeId {
                    kind: EventKind::NodeStarted
                }
            }
        ));
    }

    /// Wildcard-free `match` on purpose: adding a `RejectReason` variant without
    /// sampling it here must stop compiling.
    fn every_reject_reason() -> Vec<RejectReason> {
        let all = vec![
            RejectReason::MissingNodeId {
                kind: EventKind::NodeCompleted,
            },
            RejectReason::RunNotLive {
                run_id: "run-1".into(),
                status: RunStatus::Failed,
                node_id: "worker".into(),
                iter: 1,
                kind: EventKind::NodeCompleted,
            },
            RejectReason::ConcurrentIterationLive {
                node_id: "worker".into(),
                live_iter: 2,
                iter: 3,
            },
            RejectReason::IterationAlreadyCompleted {
                node_id: "worker".into(),
                iter: 1,
            },
            RejectReason::IterationNotCompletable {
                node_id: "worker".into(),
                iter: 1,
                status: NodeStatus::Stopped,
            },
            RejectReason::IterationNeverStarted {
                node_id: "worker".into(),
                iter: 1,
            },
        ];

        let mut seen = std::collections::BTreeSet::new();
        for r in &all {
            let key = match r {
                RejectReason::MissingNodeId { .. } => "MissingNodeId",
                RejectReason::RunNotLive { .. } => "RunNotLive",
                RejectReason::ConcurrentIterationLive { .. } => "ConcurrentIterationLive",
                RejectReason::IterationAlreadyCompleted { .. } => "IterationAlreadyCompleted",
                RejectReason::IterationNotCompletable { .. } => "IterationNotCompletable",
                RejectReason::IterationNeverStarted { .. } => "IterationNeverStarted",
            };
            seen.insert(key);
        }
        assert_eq!(
            seen.len(),
            all.len(),
            "every_reject_reason() must hold exactly one sample per variant"
        );
        all
    }

    #[test]
    fn every_reject_reason_display_is_byte_identical_to_pre_515_prose() {
        // Wire neutrality: an em-dash flipped to a hyphen, a lost `{:?}` or a
        // reordered clause each fails here.
        let cases: Vec<(RejectReason, &str)> = vec![
            (
                RejectReason::ConcurrentIterationLive {
                    node_id: "worker".into(),
                    live_iter: 2,
                    iter: 3,
                },
                "node worker iter 2 is still live: refusing concurrent iter 3",
            ),
            (
                RejectReason::RunNotLive {
                    run_id: "run-1".into(),
                    status: RunStatus::Failed,
                    node_id: "worker".into(),
                    iter: 1,
                    kind: EventKind::NodeCompleted,
                },
                "run run-1 is Failed: cannot complete node worker iter 1 — resume the run first",
            ),
            (
                RejectReason::RunNotLive {
                    run_id: "run-1".into(),
                    status: RunStatus::Failed,
                    node_id: "worker".into(),
                    iter: 1,
                    kind: EventKind::NodeStarted,
                },
                "run run-1 is Failed: no scheduling on a non-running run — resume the run first",
            ),
            (
                RejectReason::IterationNotCompletable {
                    node_id: "worker".into(),
                    iter: 1,
                    status: NodeStatus::Stopped,
                },
                "node worker iter 1 is Stopped: cannot complete",
            ),
            (
                RejectReason::IterationNeverStarted {
                    node_id: "worker".into(),
                    iter: 1,
                },
                "node worker iter 1 was never started (no node_started event): cannot complete",
            ),
            (
                RejectReason::IterationAlreadyCompleted {
                    node_id: "worker".into(),
                    iter: 1,
                },
                "node worker iter 1 already completed: refusing to re-run it",
            ),
            (
                RejectReason::MissingNodeId {
                    kind: EventKind::NodeCompleted,
                },
                "completion event without node_id",
            ),
            (
                RejectReason::MissingNodeId {
                    kind: EventKind::NodeStarted,
                },
                "start event without node_id",
            ),
            (
                RejectReason::MissingNodeId {
                    kind: EventKind::NodeFailed,
                },
                "fail event without node_id",
            ),
            (
                RejectReason::MissingNodeId {
                    kind: EventKind::NodeStale,
                },
                "stale event without node_id",
            ),
        ];
        for (reason, want) in cases {
            assert_eq!(reason.to_string(), want);
        }
        assert_eq!(every_reject_reason().len(), 6);
    }
}
