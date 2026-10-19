//! The **Attente déclarée** rule (#810, ADR-0069), in one place: Stats ›
//! Performance subtracts it from the wall-clock, and the Run projection exposes
//! it per iteration ([`crate::domain::state::IterationInfo::waited_ms`] and
//! `open_wait_since`), so the node panel and the report always agree. The
//! lifecycle telemetry reads the same causes ([`opens_declared_wait`]).

use std::collections::HashMap;

use serde_json::Value;

use crate::domain::event::{Event, EventKind};
use crate::domain::state::{AWAITING_CAUSE_COMPLETION_NOT_RELEASED, AWAITING_CAUSE_DECLARED};

/// The cause a `NodeAwaitingUser` carries. `declared_wait` writes `cause`; the
/// orchestrator binding's marker writes `reason` (ADR-0064) — read both keys.
/// `None` = an event with no cause at all: **ignored**, never a wait. Only a
/// pre-#810 spawn event could be in that shape (the primitive no longer writes
/// one), and counting it as a wait would subtract time nobody waited.
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

fn wait_millis(opened_at: &str, closed_at: &str) -> Option<f64> {
    let start = chrono::DateTime::parse_from_rfc3339(opened_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(closed_at).ok()?;
    Some((end - start).num_milliseconds() as f64)
}

/// Fed the events of one Run in order, accumulates per execution
/// `(node_id, iter)` the **closed** declared waits, in milliseconds:
/// - an interval opens on a `NodeAwaitingUser` of cause `declared` or
///   `completion_not_released` (`children_pending` excluded);
/// - it closes on the FIRST `NodeResumed`, `NodeCompletionReleased` or terminal
///   event of the same key. A repeated `NodeAwaitingUser` with no lift in
///   between does not reopen it (a refreshed question is the same wait);
/// - an interval still open contributes nothing: that execution has not
///   finished waiting ([`Self::open_since`] says since when; the node panel
///   adds the open wait itself);
/// - a `NodeStarted` on a key **resets** it: a stopped-then-restarted attempt
///   runs at the same `(node, iter)`, and the wait of the attempt that was
///   thrown away is not the wait of the one that succeeded.
#[derive(Debug, Default)]
pub struct DeclaredWaitLedger {
    open: HashMap<(String, i64), String>,
    waited: HashMap<(String, i64), f64>,
}

impl DeclaredWaitLedger {
    pub fn observe(&mut self, event: &Event) {
        let Some(node_id) = event.node_id.clone() else {
            return;
        };
        let key = (node_id, event.iter.unwrap_or(1));
        match event.kind {
            EventKind::NodeStarted => {
                self.open.remove(&key);
                self.waited.remove(&key);
            }
            EventKind::NodeAwaitingUser => {
                let Some(cause) = awaiting_cause(event.payload.as_ref()) else {
                    return;
                };
                if !opens_declared_wait(cause) {
                    return;
                }
                self.open.entry(key).or_insert_with(|| event.ts.clone());
            }
            EventKind::NodeResumed
            | EventKind::NodeCompletionReleased
            | EventKind::NodeCompleted
            | EventKind::NodeAutoCompleted
            | EventKind::NodeFailed
            | EventKind::NodeStopped
            | EventKind::NodeInterrupted
            | EventKind::NodeStale => {
                if let Some(opened_at) = self.open.remove(&key) {
                    if let Some(millis) = wait_millis(&opened_at, &event.ts) {
                        *self.waited.entry(key).or_insert(0.0) += millis.max(0.0);
                    }
                }
            }
            _ => {}
        }
    }

    /// The closed declared wait of one execution, in milliseconds (`0` when none).
    pub fn waited_ms(&self, node_id: &str, iter: i64) -> f64 {
        self.waited
            .get(&(node_id.to_string(), iter))
            .copied()
            .unwrap_or(0.0)
    }

    /// When the declared wait still open on one execution began — the FIRST
    /// marker of that wait — or `None`.
    pub fn open_since(&self, node_id: &str, iter: i64) -> Option<&str> {
        self.open
            .get(&(node_id.to_string(), iter))
            .map(String::as_str)
    }

    pub fn into_waits(self) -> HashMap<(String, i64), f64> {
        self.waited
    }
}

/// The closed declared wait of every execution `(node_id, iter)` of one Run, in
/// milliseconds — see [`DeclaredWaitLedger`] for the rule.
pub fn declared_waits(events: &[Event]) -> HashMap<(String, i64), f64> {
    let mut ledger = DeclaredWaitLedger::default();
    for event in events {
        ledger.observe(event);
    }
    ledger.into_waits()
}
