//! Putting long-idle waits to sleep (#1009 follow-up).
//!
//! A node that asked its user a question (`pdo wait-user`) keeps a whole agent
//! session alive for as long as the question stays unanswered — days, sometimes,
//! for a grilling left open. Each one holds a couple of hundred MB on a shared
//! machine while doing nothing. Past [`DEFAULT_HIBERNATE_AFTER_MINUTES`] the daemon
//! snapshots the pane, closes the session and records `NodeHibernated`; the agent's
//! transcript stays on disk. Opening the node wakes it: the session is resumed on
//! its pinned session id (ADR-0049 §3a) and `NodeWoken` is recorded. The wait itself
//! never moves — the node stays `AwaitingUser` with its question the whole time.
//!
//! Only declared waits sleep: a wait on children, an incident or a refused
//! completion has its own lifecycle and is left alone.

use chrono::{DateTime, Duration, Utc};

use crate::event_log::{NodeStatus, RunState, AWAITING_CAUSE_DECLARED};

/// Env var: minutes a declared wait stays unanswered before its session is put to
/// sleep. `0` turns hibernation off.
pub(crate) const HIBERNATE_AFTER_ENV: &str = "PDO_HIBERNATE_AFTER_MINUTES";

/// Default delay before a declared wait's session is put to sleep. Long enough
/// that a question answered within the working session is never interrupted;
/// short enough that one left for the afternoon gives its memory back.
pub(crate) const DEFAULT_HIBERNATE_AFTER_MINUTES: i64 = 120;

/// The configured delay, from [`HIBERNATE_AFTER_ENV`], else the default. `None` =
/// hibernation off.
pub(crate) fn configured_hibernate_after() -> Option<Duration> {
    hibernate_after_from(std::env::var(HIBERNATE_AFTER_ENV).ok().as_deref())
}

/// [`configured_hibernate_after`] over an explicit env value (pure). `0` turns it
/// off; an unparseable or negative value keeps the default rather than silently
/// changing behaviour.
pub(crate) fn hibernate_after_from(env: Option<&str>) -> Option<Duration> {
    match env.map(str::trim).map(str::parse::<i64>) {
        Some(Ok(0)) => None,
        Some(Ok(minutes)) if minutes > 0 => Some(Duration::minutes(minutes)),
        _ => Some(Duration::minutes(DEFAULT_HIBERNATE_AFTER_MINUTES)),
    }
}

/// The `(node_id, iter)` of `run` whose declared wait has lasted at least `after`
/// at `now` and is still awake. Sorted, for a deterministic sweep. Whether the
/// session can actually be resumed later is the caller's check — this is only
/// about the wait.
pub(crate) fn due_for_hibernation(
    run: &RunState,
    now: DateTime<Utc>,
    after: Duration,
) -> Vec<(String, i64)> {
    let mut due: Vec<(String, i64)> = run
        .nodes
        .values()
        .filter(|node| node.status == NodeStatus::AwaitingUser)
        .filter(|node| {
            node.awaiting.as_ref().is_some_and(|wait| {
                wait.cause == AWAITING_CAUSE_DECLARED
                    && !wait.hibernated
                    && DateTime::parse_from_rfc3339(&wait.since)
                        .is_ok_and(|since| now - since.with_timezone(&Utc) >= after)
            })
        })
        .map(|node| (node.node_id.clone(), node.iter))
        .collect();
    due.sort();
    due
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{project, Event, EventKind};

    fn at(minutes: i64) -> String {
        (base() + Duration::minutes(minutes)).to_rfc3339()
    }

    fn base() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-05T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn event(
        kind: EventKind,
        node: Option<&str>,
        minutes: i64,
        payload: Option<serde_json::Value>,
    ) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: at(minutes),
            kind,
            node_id: node.map(str::to_string),
            iter: node.map(|_| 1),
            payload,
        }
    }

    /// A run whose `asker` node declared a wait (cause `cause`) at minute 0.
    fn run_with_wait(cause: &str, extra: Vec<Event>) -> RunState {
        let mut events = vec![
            event(
                EventKind::RunStarted,
                None,
                -10,
                Some(serde_json::json!({ "pipeline_name": "grill" })),
            ),
            event(EventKind::NodeStarted, Some("asker"), -5, None),
            event(
                EventKind::NodeAwaitingUser,
                Some("asker"),
                0,
                Some(serde_json::json!({ "cause": cause, "message": "Q7 ?" })),
            ),
        ];
        events.extend(extra);
        project(&events).unwrap()
    }

    #[test]
    fn delay_defaults_to_two_hours_and_zero_turns_it_off() {
        assert_eq!(hibernate_after_from(None), Some(Duration::minutes(120)));
        assert_eq!(
            hibernate_after_from(Some("30")),
            Some(Duration::minutes(30))
        );
        assert_eq!(hibernate_after_from(Some("0")), None);
        assert_eq!(
            hibernate_after_from(Some("2h")),
            Some(Duration::minutes(120))
        );
        assert_eq!(
            hibernate_after_from(Some("-5")),
            Some(Duration::minutes(120))
        );
    }

    #[test]
    fn a_declared_wait_is_due_once_the_delay_has_passed() {
        let run = run_with_wait("declared", Vec::new());
        let after = Duration::minutes(120);

        assert!(due_for_hibernation(&run, base() + Duration::minutes(119), after).is_empty());
        assert_eq!(
            due_for_hibernation(&run, base() + Duration::minutes(120), after),
            vec![("asker".to_string(), 1)]
        );
    }

    #[test]
    fn only_declared_waits_sleep() {
        let run = run_with_wait("children_pending", Vec::new());
        let late = base() + Duration::days(3);

        assert!(due_for_hibernation(&run, late, Duration::minutes(120)).is_empty());
    }

    #[test]
    fn a_sleeping_wait_is_not_due_again_and_a_woken_one_is() {
        let late = base() + Duration::days(1);
        let after = Duration::minutes(120);
        let asleep = run_with_wait(
            "declared",
            vec![event(EventKind::NodeHibernated, Some("asker"), 130, None)],
        );
        assert!(asleep.nodes["asker"].awaiting.as_ref().unwrap().hibernated);
        assert!(due_for_hibernation(&asleep, late, after).is_empty());

        let woken = run_with_wait(
            "declared",
            vec![
                event(EventKind::NodeHibernated, Some("asker"), 130, None),
                event(EventKind::NodeWoken, Some("asker"), 200, None),
            ],
        );
        let wait = woken.nodes["asker"].awaiting.as_ref().unwrap();
        // Woken keeps the wait and its question: only the flag moved.
        assert!(!wait.hibernated);
        assert_eq!(wait.message.as_deref(), Some("Q7 ?"));
        assert_eq!(woken.nodes["asker"].status, NodeStatus::AwaitingUser);
        assert_eq!(due_for_hibernation(&woken, late, after).len(), 1);
    }

    #[test]
    fn an_answer_clears_the_sleep_with_the_wait() {
        let run = run_with_wait(
            "declared",
            vec![
                event(EventKind::NodeHibernated, Some("asker"), 130, None),
                event(EventKind::NodeResumed, Some("asker"), 140, None),
            ],
        );

        assert_eq!(run.nodes["asker"].status, NodeStatus::Running);
        assert!(run.nodes["asker"].awaiting.is_none());
    }

    #[test]
    fn a_hibernation_of_a_node_no_longer_waiting_is_a_no_op() {
        let run = run_with_wait(
            "declared",
            vec![
                event(EventKind::NodeResumed, Some("asker"), 10, None),
                event(EventKind::NodeHibernated, Some("asker"), 130, None),
            ],
        );

        assert_eq!(run.nodes["asker"].status, NodeStatus::Running);
        assert!(run.nodes["asker"].awaiting.is_none());
    }
}
