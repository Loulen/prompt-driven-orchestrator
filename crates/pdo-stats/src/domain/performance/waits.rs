//! Declared wait and active duration (#810, ADR-0069): what opens and closes a
//! wait, and the three readings of one wall-clock.

use std::collections::HashMap;

use serde_json::Value;

use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::state::{AWAITING_CAUSE_COMPLETION_NOT_RELEASED, AWAITING_CAUSE_DECLARED};

pub fn parse_ts(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(ts).ok()
}

pub fn duration_millis(started_at: &str, completed_at: &str) -> Option<f64> {
    let start = parse_ts(started_at)?;
    let end = parse_ts(completed_at)?;
    Some((end - start).num_milliseconds() as f64)
}

// --- Declared wait / active duration (#810, ADR-0069) --------------------------

/// The cause a `NodeAwaitingUser` carries. `declared_wait` writes `cause`; the
/// orchestrator binding's marker writes `reason` (ADR-0064) — the projection
/// reads both keys, and so does this. `None` = an event with no cause at all:
/// **ignored**, never a wait. Only a pre-#810 spawn event could be in that
/// shape (the primitive no longer writes one), and projecting it as a wait
/// would subtract time nobody waited.
pub fn awaiting_cause(payload: Option<&Value>) -> Option<&str> {
    payload
        .and_then(|p| p.get("cause").or_else(|| p.get("reason")))
        .and_then(|v| v.as_str())
}

/// Does this cause open a declared wait? `children_pending` does not: an
/// orchestrator parked on `pdo run wait` is working, not waiting on a human
/// (CONTEXT.md § Durée active). Nor does `child_awaiting`, which is derived at
/// read and never written to the log.
pub fn opens_declared_wait(cause: &str) -> bool {
    cause == AWAITING_CAUSE_DECLARED || cause == AWAITING_CAUSE_COMPLETION_NOT_RELEASED
}

/// The declared wait of every execution `(node_id, iter)` of one Run, in
/// milliseconds — what [`StatsHarnessPerformance::active_duration`] subtracts
/// from the wall-clock.
///
/// An interval opens on a `NodeAwaitingUser` of cause `declared` or
/// `completion_not_released` and closes on the FIRST `NodeResumed`,
/// `NodeCompletionReleased` or terminal event of the same key. A repeated
/// `NodeAwaitingUser` with no lift in between does not reopen it (a refreshed
/// question is the same wait). An interval still open when the log ends
/// contributes nothing — that execution never completed, so it is not an
/// observation either.
///
/// A `NodeStarted` on a key **resets** it: a stopped-then-restarted attempt
/// runs at the same `(node, iter)`, and the wait of the attempt that was thrown
/// away is not the wait of the one that succeeded. The Infrastructure row sums
/// these same per-execution waits, so a run's active duration always agrees
/// with the nodes shown under it.
pub fn declared_waits(events: &[Event]) -> HashMap<(String, i64), f64> {
    let mut open: HashMap<(String, i64), String> = HashMap::new();
    let mut waited: HashMap<(String, i64), f64> = HashMap::new();
    for event in events {
        let Some(node_id) = event.node_id.clone() else {
            continue;
        };
        let key = (node_id, event.iter.unwrap_or(1));
        match event.kind {
            EventKind::NodeStarted => {
                open.remove(&key);
                waited.remove(&key);
            }
            EventKind::NodeAwaitingUser => {
                let Some(cause) = awaiting_cause(event.payload.as_ref()) else {
                    continue;
                };
                if !opens_declared_wait(cause) {
                    continue;
                }
                open.entry(key).or_insert_with(|| event.ts.clone());
            }
            EventKind::NodeResumed
            | EventKind::NodeCompletionReleased
            | EventKind::NodeCompleted
            | EventKind::NodeAutoCompleted
            | EventKind::NodeFailed
            | EventKind::NodeStopped
            | EventKind::NodeInterrupted
            | EventKind::NodeStale => {
                if let Some(started_at) = open.remove(&key) {
                    if let Some(millis) = duration_millis(&started_at, &event.ts) {
                        *waited.entry(key).or_insert(0.0) += millis.max(0.0);
                    }
                }
            }
            _ => {}
        }
    }
    waited
}

/// One execution's wall-clock minus its declared wait, floored at zero (a clock
/// skew between two events must not produce a negative duration).
pub fn active_millis(duration: Option<f64>, waited: f64) -> Option<f64> {
    duration.map(|value| (value - waited).max(0.0))
}

#[cfg(test)]
mod tests {
    //! Direct tests of the wait fold (#819) — the interval construction and the
    //! three readings it feeds. Until now both were reachable only through the
    //! HTTP handler, which needs a database, a forged event log and a
    //! transcript corpus to say anything about a subtraction of two timestamps.
    //! These sit on the same seam the handler uses, one level below it.
    use super::super::acc::{DurationReading, HarnessAcc};
    use super::*;
    use pdo_run::domain::state::AWAITING_CAUSE_CHILD_AWAITING;
    use pretty_assertions::assert_eq;

    const MIN: f64 = 60_000.0;

    fn event(ts: &str, kind: EventKind, node: &str, payload: Value) -> Event {
        Event {
            id: None,
            run_id: "r1".to_string(),
            ts: format!("2033-03-10T{ts}:00Z"),
            kind,
            node_id: Some(node.to_string()),
            iter: Some(1),
            payload: Some(payload),
        }
    }

    fn awaiting(ts: &str, node: &str, cause: &str) -> Event {
        event(
            ts,
            EventKind::NodeAwaitingUser,
            node,
            serde_json::json!({ "cause": cause }),
        )
    }

    fn waited(events: &[Event], node: &str) -> f64 {
        declared_waits(events)
            .get(&(node.to_string(), 1))
            .copied()
            .unwrap_or(0.0)
    }

    #[test]
    fn a_declared_wait_runs_from_the_question_to_the_first_lift() {
        let events = [
            event("10:00", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:01", "chat", AWAITING_CAUSE_DECLARED),
            event("10:04", EventKind::NodeResumed, "chat", Value::Null),
            event("10:10", EventKind::NodeCompleted, "chat", Value::Null),
        ];
        assert_eq!(waited(&events, "chat"), 3.0 * MIN);
    }

    #[test]
    fn two_waits_of_one_execution_add_up_whatever_lifts_them() {
        let events = [
            event("10:00", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:01", "chat", AWAITING_CAUSE_DECLARED),
            event("10:03", EventKind::NodeResumed, "chat", Value::Null),
            awaiting("10:04", "chat", AWAITING_CAUSE_COMPLETION_NOT_RELEASED),
            event(
                "10:05",
                EventKind::NodeCompletionReleased,
                "chat",
                Value::Null,
            ),
            event("10:10", EventKind::NodeCompleted, "chat", Value::Null),
        ];
        assert_eq!(waited(&events, "chat"), 3.0 * MIN);
    }

    #[test]
    fn a_repeated_question_with_no_lift_in_between_is_still_one_wait() {
        // A refreshed question is the same wait: the interval opens once, at the
        // FIRST marker, and closes at the first lift — never restarts its clock.
        let events = [
            event("10:00", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:01", "chat", AWAITING_CAUSE_DECLARED),
            awaiting("10:03", "chat", AWAITING_CAUSE_DECLARED),
            event("10:05", EventKind::NodeResumed, "chat", Value::Null),
        ];
        assert_eq!(waited(&events, "chat"), 4.0 * MIN);
    }

    #[test]
    fn a_terminal_event_closes_a_wait_that_was_never_lifted() {
        let events = [
            event("10:00", EventKind::NodeStarted, "late", Value::Null),
            awaiting("10:02", "late", AWAITING_CAUSE_DECLARED),
            event("10:12", EventKind::NodeCompleted, "late", Value::Null),
        ];
        assert_eq!(waited(&events, "late"), 10.0 * MIN);
    }

    #[test]
    fn an_interval_still_open_when_the_log_ends_contributes_nothing() {
        // That execution never completed, so it is not an observation either.
        let events = [
            event("10:00", EventKind::NodeStarted, "open", Value::Null),
            awaiting("10:02", "open", AWAITING_CAUSE_DECLARED),
        ];
        assert_eq!(waited(&events, "open"), 0.0);
    }

    #[test]
    fn children_pending_and_child_awaiting_open_nothing() {
        // An orchestrator parked on `pdo run wait` is working; a derived child
        // wait is never written to the log in the first place (ADR-0069 §4).
        for cause in ["children_pending", AWAITING_CAUSE_CHILD_AWAITING] {
            let events = [
                event("10:00", EventKind::NodeStarted, "orch", Value::Null),
                awaiting("10:02", "orch", cause),
                event("10:06", EventKind::NodeCompleted, "orch", Value::Null),
            ];
            assert_eq!(waited(&events, "orch"), 0.0, "cause {cause}");
        }
    }

    #[test]
    fn an_awaiting_marker_with_no_cause_at_all_is_ignored() {
        let events = [
            event("10:00", EventKind::NodeStarted, "plain", Value::Null),
            event(
                "10:01",
                EventKind::NodeAwaitingUser,
                "plain",
                serde_json::json!({}),
            ),
            event("10:04", EventKind::NodeCompleted, "plain", Value::Null),
        ];
        assert_eq!(waited(&events, "plain"), 0.0);
    }

    #[test]
    fn the_orchestrator_bindings_reason_key_is_read_like_a_cause() {
        let events = [
            event("10:00", EventKind::NodeStarted, "orch", Value::Null),
            event(
                "10:02",
                EventKind::NodeAwaitingUser,
                "orch",
                serde_json::json!({"reason":"children_pending","active":["c1"]}),
            ),
            event("10:06", EventKind::NodeCompleted, "orch", Value::Null),
        ];
        assert_eq!(waited(&events, "orch"), 0.0);
    }

    #[test]
    fn a_restart_at_the_same_key_throws_away_the_abandoned_attempts_wait() {
        // The wait of the attempt that was thrown away is not the wait of the
        // one that succeeded.
        let events = [
            event("10:00", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:01", "chat", AWAITING_CAUSE_DECLARED),
            event("10:06", EventKind::NodeStopped, "chat", Value::Null),
            event("10:07", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:08", "chat", AWAITING_CAUSE_DECLARED),
            event("10:09", EventKind::NodeResumed, "chat", Value::Null),
            event("10:12", EventKind::NodeCompleted, "chat", Value::Null),
        ];
        assert_eq!(waited(&events, "chat"), 1.0 * MIN);
    }

    #[test]
    fn each_execution_of_the_same_node_keeps_its_own_wait() {
        let mut events = vec![
            event("10:00", EventKind::NodeStarted, "chat", Value::Null),
            awaiting("10:01", "chat", AWAITING_CAUSE_DECLARED),
            event("10:03", EventKind::NodeResumed, "chat", Value::Null),
            event("10:05", EventKind::NodeCompleted, "chat", Value::Null),
        ];
        // The second lap of the very same Node: another `(node, iter)` key.
        for offset in ["10:06", "10:07", "10:11", "10:12"] {
            let mut lap2 = match offset {
                "10:06" => event(offset, EventKind::NodeStarted, "chat", Value::Null),
                "10:07" => awaiting(offset, "chat", AWAITING_CAUSE_DECLARED),
                "10:11" => event(offset, EventKind::NodeResumed, "chat", Value::Null),
                _ => event(offset, EventKind::NodeCompleted, "chat", Value::Null),
            };
            lap2.iter = Some(2);
            events.push(lap2);
        }
        let waits = declared_waits(&events);
        assert_eq!(
            waits.get(&("chat".to_string(), 1)).copied(),
            Some(2.0 * MIN)
        );
        assert_eq!(
            waits.get(&("chat".to_string(), 2)).copied(),
            Some(4.0 * MIN)
        );
    }

    #[test]
    fn the_three_readings_split_one_wall_clock_in_two() {
        let reading = DurationReading::new(Some(10.0 * MIN), 3.0 * MIN, None);
        assert_eq!(reading.total, Some(10.0 * MIN));
        assert_eq!(reading.active, Some(7.0 * MIN));
        assert_eq!(reading.waiting, Some(3.0 * MIN));
        assert_eq!(
            reading.active.unwrap() + reading.waiting.unwrap(),
            reading.total.unwrap(),
            "Total = Active + Waiting, execution by execution"
        );
    }

    #[test]
    fn an_execution_nobody_waited_on_measures_a_waiting_of_zero() {
        // Zero is data, not an absence: the row plots a flat box at 0 rather
        // than « no data », and the coverage stays the wall-clock's.
        let reading = DurationReading::new(Some(4.0 * MIN), 0.0, None);
        assert_eq!(reading.waiting, Some(0.0));
        assert_eq!(reading.active, reading.total);
    }

    #[test]
    fn a_wait_longer_than_the_wall_clock_clamps_without_breaking_the_identity() {
        // A clock skew must not produce a negative Active, nor a Waiting the
        // Total cannot account for.
        let reading = DurationReading::new(Some(2.0 * MIN), 5.0 * MIN, None);
        assert_eq!(reading.active, Some(0.0));
        assert_eq!(reading.waiting, Some(2.0 * MIN));
        assert_eq!(
            reading.active.unwrap() + reading.waiting.unwrap(),
            reading.total.unwrap()
        );
    }

    #[test]
    fn an_unmeasurable_wall_clock_leaves_the_three_readings_absent_together() {
        // The coverage must never disagree between modes: switching mode cannot
        // change how many executions the reader is looking at.
        let reading = DurationReading::new(None, 3.0 * MIN, Some("unparseable timestamp"));
        assert_eq!(reading.total, None);
        assert_eq!(reading.active, None);
        assert_eq!(reading.waiting, None);

        let mut acc = HarnessAcc::default();
        acc.observe_durations(reading);
        let finished = acc.finish("claude".to_string());
        for metric in [
            &finished.duration,
            &finished.active_duration,
            &finished.wait_duration,
        ] {
            assert_eq!(metric.measured, 0);
            assert_eq!(metric.expected, 1);
            assert_eq!(metric.missing_reasons, vec!["unparseable timestamp"]);
        }
    }

    #[test]
    fn an_unwaited_reading_is_the_wall_clock_and_a_zero() {
        let reading = DurationReading::unwaited(Some(90_000.0), None);
        assert_eq!(reading.active, Some(90_000.0));
        assert_eq!(reading.waiting, Some(0.0));
    }
}
