//! The Presences on the Runs (API 0.12): which Actors have a Run open in a
//! browser, from what each connection of the event stream says it shows.
//!
//! A connection shows one Run at most. An Actor is present on a Run while one
//! of its connections at least shows it: it **arrives** when its first one
//! does, and **leaves** when its last one stops (shows another Run, nothing,
//! or closes). Nothing here does I/O: the clock is handed in.

use std::collections::HashMap;

use pdo_extension_api::Presence;
use pdo_kernel::Actor;

/// The id of one connection of the event stream, unique for the daemon's life.
pub type ConnectionId = u64;

/// An arrival or a departure, to announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceChange {
    Arrived { run_id: String, actor: Actor },
    Left { run_id: String, actor: Actor },
}

impl PresenceChange {
    pub fn run_id(&self) -> &str {
        match self {
            PresenceChange::Arrived { run_id, .. } | PresenceChange::Left { run_id, .. } => run_id,
        }
    }
}

/// What a connection shows, and under which Actor.
#[derive(Debug, Clone)]
struct Seat {
    actor: Actor,
    run_id: String,
}

/// The Presences of every Run.
#[derive(Debug, Default)]
pub struct PresenceBook {
    /// The connections showing a Run.
    seats: HashMap<ConnectionId, Seat>,
    /// Per Run, the Actors present and when each arrived, by order of
    /// arrival.
    runs: HashMap<String, Vec<Presence>>,
}

impl PresenceBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the connection `connection`, under `actor`, already shows
    /// `run_id` (or no Run): showing it again changes nothing.
    pub fn shows(&self, connection: ConnectionId, actor: &Actor, run_id: Option<&str>) -> bool {
        match self.seats.get(&connection) {
            Some(seat) => Some(seat.run_id.as_str()) == run_id && &seat.actor == actor,
            None => run_id.is_none(),
        }
    }

    /// The connection `connection`, under `actor`, now shows `run_id` (or no
    /// Run). What it changes, departure first.
    pub fn show(
        &mut self,
        connection: ConnectionId,
        actor: &Actor,
        run_id: Option<&str>,
        now: &str,
    ) -> Vec<PresenceChange> {
        if self.shows(connection, actor, run_id) {
            return Vec::new();
        }
        let mut changes = self.close(connection);
        if let Some(run_id) = run_id {
            let arrives = !self.shown_by(actor, run_id);
            self.seats.insert(
                connection,
                Seat {
                    actor: actor.clone(),
                    run_id: run_id.to_string(),
                },
            );
            if arrives {
                self.runs
                    .entry(run_id.to_string())
                    .or_default()
                    .push(Presence::new(actor.clone(), now));
                changes.push(PresenceChange::Arrived {
                    run_id: run_id.to_string(),
                    actor: actor.clone(),
                });
            }
        }
        changes
    }

    /// The connection `connection` ended (or shows nothing any more): the
    /// departure it causes, if any.
    pub fn close(&mut self, connection: ConnectionId) -> Vec<PresenceChange> {
        let Some(seat) = self.seats.remove(&connection) else {
            return Vec::new();
        };
        if self.shown_by(&seat.actor, &seat.run_id) {
            return Vec::new();
        }
        if let Some(present) = self.runs.get_mut(&seat.run_id) {
            present.retain(|presence| presence.actor != seat.actor);
            if present.is_empty() {
                self.runs.remove(&seat.run_id);
            }
        }
        vec![PresenceChange::Left {
            run_id: seat.run_id,
            actor: seat.actor,
        }]
    }

    /// The Run `run_id` is forgotten: nobody is present on it any more, and
    /// nothing is announced (its `run_forgotten` says it all).
    pub fn forget_run(&mut self, run_id: &str) {
        self.seats.retain(|_, seat| seat.run_id != run_id);
        self.runs.remove(run_id);
    }

    /// The Actors present on `run_id`, by order of arrival.
    pub fn presences(&self, run_id: &str) -> Vec<Presence> {
        self.runs.get(run_id).cloned().unwrap_or_default()
    }

    /// Every Run someone has open, with its Presences, by Run id.
    pub fn all(&self) -> Vec<(String, Vec<Presence>)> {
        let mut all: Vec<_> = self
            .runs
            .iter()
            .map(|(run_id, present)| (run_id.clone(), present.clone()))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// Whether a connection of `actor` shows `run_id`.
    fn shown_by(&self, actor: &Actor, run_id: &str) -> bool {
        self.seats
            .values()
            .any(|seat| &seat.actor == actor && seat.run_id == run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str) -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: id.into(),
        }
    }

    fn arrived(run_id: &str, actor: &Actor) -> PresenceChange {
        PresenceChange::Arrived {
            run_id: run_id.into(),
            actor: actor.clone(),
        }
    }

    fn left(run_id: &str, actor: &Actor) -> PresenceChange {
        PresenceChange::Left {
            run_id: run_id.into(),
            actor: actor.clone(),
        }
    }

    fn actors(book: &PresenceBook, run_id: &str) -> Vec<Actor> {
        book.presences(run_id)
            .into_iter()
            .map(|p| p.actor)
            .collect()
    }

    #[test]
    fn two_actors_on_one_run_are_two_presences_in_order_of_arrival() {
        let (alice, bob) = (person("alice"), person("bob"));
        let mut book = PresenceBook::new();
        assert_eq!(
            book.show(1, &alice, Some("r1"), "t1"),
            [arrived("r1", &alice)]
        );
        assert_eq!(book.show(2, &bob, Some("r1"), "t2"), [arrived("r1", &bob)]);
        assert_eq!(actors(&book, "r1"), [alice.clone(), bob.clone()]);
        assert_eq!(book.presences("r1")[0].since, "t1");
        assert_eq!(book.close(1), [left("r1", &alice)]);
        assert_eq!(actors(&book, "r1"), [bob]);
    }

    #[test]
    fn two_tabs_of_one_actor_are_one_presence_until_the_last_leaves() {
        let alice = person("alice");
        let mut book = PresenceBook::new();
        book.show(1, &alice, Some("r1"), "t1");
        assert_eq!(book.show(2, &alice, Some("r1"), "t2"), []);
        assert_eq!(book.presences("r1").len(), 1);
        assert_eq!(
            book.presences("r1")[0].since,
            "t1",
            "the first tab's arrival"
        );
        assert_eq!(book.close(1), []);
        assert_eq!(book.close(2), [left("r1", &alice)]);
        assert!(book.presences("r1").is_empty());
        assert!(book.all().is_empty());
    }

    #[test]
    fn showing_another_run_leaves_the_first_then_arrives_on_the_second() {
        let alice = person("alice");
        let mut book = PresenceBook::new();
        book.show(1, &alice, Some("r1"), "t1");
        assert_eq!(
            book.show(1, &alice, Some("r2"), "t2"),
            [left("r1", &alice), arrived("r2", &alice)]
        );
        assert_eq!(
            book.show(1, &alice, Some("r2"), "t3"),
            [],
            "nothing changes"
        );
        assert_eq!(book.show(1, &alice, None, "t4"), [left("r2", &alice)]);
        assert_eq!(
            book.close(1),
            [],
            "a connection showing nothing leaves nothing"
        );
    }

    #[test]
    fn a_forgotten_run_has_nobody_on_it_and_its_tabs_leave_nothing() {
        let alice = person("alice");
        let mut book = PresenceBook::new();
        book.show(1, &alice, Some("r1"), "t1");
        assert!(book.shows(1, &alice, Some("r1")));
        book.forget_run("r1");
        assert!(book.presences("r1").is_empty());
        assert!(book.shows(1, &alice, None));
        assert_eq!(book.close(1), []);
    }

    #[test]
    fn every_open_run_is_listed_with_its_presences() {
        let (alice, bob) = (person("alice"), person("bob"));
        let mut book = PresenceBook::new();
        book.show(1, &alice, Some("r2"), "t1");
        book.show(2, &bob, Some("r1"), "t2");
        let all = book.all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, "r1");
        assert_eq!(all[1].1[0].actor, alice);
    }
}
