//! A session found dead by a sweep whose snapshot predates a restart of the same
//! iteration. `restart_node` kills the session, appends a new `NodeStarted`, then
//! spawns a session under the very same name: the death the sweep saw is the
//! restart's own kill. Acting on it would interrupt the restarted iteration and
//! reap — kill — its new session.

use crate::application::events::load_events;
use crate::domain::event::{Event, EventKind};
use crate::Runs;

fn started(events: &[Event], node_id: &str, iter: i64) -> usize {
    events
        .iter()
        .filter(|e| {
            e.kind == EventKind::NodeStarted
                && e.node_id.as_deref() == Some(node_id)
                && e.iter == Some(iter)
        })
        .count()
}

/// Whether `(node_id, iter)` was started again in `fresh` since `snapshot`.
pub(super) fn restarted_since(
    snapshot: &[Event],
    fresh: &[Event],
    node_id: &str,
    iter: i64,
) -> bool {
    started(fresh, node_id, iter) > started(snapshot, node_id, iter)
}

/// [`restarted_since`] against the log as it is now. A log that cannot be read
/// changes nothing: the sweep acts on its snapshot, as before.
pub(super) async fn restarted_since_snapshot(
    runs: &Runs,
    snapshot: &[Event],
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> bool {
    load_events(runs, run_id)
        .await
        .is_ok_and(|fresh| restarted_since(snapshot, &fresh, node_id, iter))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: EventKind, node_id: &str, iter: i64) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: "2026-10-31T00:00:00Z".into(),
            kind,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: None,
        }
    }

    #[test]
    fn a_new_node_started_of_the_same_iteration_is_a_restart() {
        let snapshot = vec![ev(EventKind::NodeStarted, "planner", 1)];
        let mut fresh = snapshot.clone();
        fresh.push(ev(EventKind::CommandIssued, "planner", 1));
        assert!(!restarted_since(&snapshot, &fresh, "planner", 1));
        fresh.push(ev(EventKind::NodeStarted, "planner", 1));
        assert!(restarted_since(&snapshot, &fresh, "planner", 1));
    }

    #[test]
    fn another_node_or_another_iteration_is_not_this_one_restarted() {
        let snapshot = vec![ev(EventKind::NodeStarted, "planner", 1)];
        let mut fresh = snapshot.clone();
        fresh.push(ev(EventKind::NodeStarted, "impl-1", 1));
        fresh.push(ev(EventKind::NodeStarted, "planner", 2));
        assert!(!restarted_since(&snapshot, &fresh, "planner", 1));
    }
}
