//! Who made a gesture on a Run, kept on its events: the audit of a Run.
//!
//! A gesture (a command of the operator or the Pipeline Manager, a retry, a
//! stop, a skip, a completion, a release…) is handled [`by`] its Actor, the
//! one the authorization was asked about. Every event of **that Run** the
//! gesture appends while it is handled, its own and those it causes inline
//! (the stop and the respawn of a retry, the downstream a mark done starts),
//! carries that Actor as `payload.actor` ([`Event::with_actor`]). The single
//! point that writes it is [`AppendEvent`](super::append_event::AppendEvent),
//! so no emitter has to thread the Actor down to its event.
//!
//! What the gesture hands to a detached task (a terminal tail, a scheduler
//! pass spawned apart), and the events of other Runs it touches (re-driving
//! the throttled nodes of every Run), are not its own: they carry no Actor.
//! An event that names its Actor already keeps it.

use std::future::Future;

use pdo_kernel::Actor;

use crate::domain::event::Event;

struct Gesture {
    run_id: String,
    actor: Actor,
}

tokio::task_local! {
    static GESTURE: Gesture;
}

/// Handle `gesture`, made by `actor` on the Run `run_id`: the events of that
/// Run it appends name `actor`.
pub async fn by<F: Future>(actor: &Actor, run_id: &str, gesture: F) -> F::Output {
    GESTURE
        .scope(
            Gesture {
                run_id: run_id.to_string(),
                actor: actor.clone(),
            },
            gesture,
        )
        .await
}

/// `event`, naming the Actor of the gesture being handled when it is an
/// event of that gesture's Run; otherwise `event` as it is.
pub(crate) fn stamped(event: &Event) -> Option<Event> {
    GESTURE
        .try_with(|gesture| {
            (gesture.run_id == event.run_id).then(|| event.clone().with_actor(&gesture.actor))
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::EventKind;

    fn event(run_id: &str) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: "2026-11-07T10:00:00Z".into(),
            kind: EventKind::NodeStopped,
            node_id: Some("n1".into()),
            iter: Some(1),
            payload: None,
        }
    }

    fn alice() -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        }
    }

    #[tokio::test]
    async fn an_event_of_the_gestures_run_names_its_actor() {
        let stamped = by(&alice(), "r1", async { stamped(&event("r1")) }).await;
        assert_eq!(stamped.and_then(|e| e.actor()), Some(alice()));
    }

    #[tokio::test]
    async fn an_event_of_another_run_names_nobody() {
        assert!(by(&alice(), "r1", async { stamped(&event("r2")) })
            .await
            .is_none());
    }

    #[tokio::test]
    async fn outside_a_gesture_nothing_is_stamped() {
        assert!(stamped(&event("r1")).is_none());
    }

    #[tokio::test]
    async fn a_detached_task_is_not_the_gesture() {
        let detached = by(&alice(), "r1", async {
            tokio::spawn(async { stamped(&event("r1")) }).await.unwrap()
        })
        .await;
        assert!(detached.is_none());
    }
}
