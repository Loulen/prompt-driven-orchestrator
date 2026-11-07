//! What crossed a terminal's bridge, per session: the typed bytes the Enter
//! rule of the declared wait counts (#588 / ADR-0069 §1), and who holds the
//! terminal and the floor, so a Run's log keeps one line per take-over and
//! per turn of speech (CORE-202).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use pdo_kernel::Actor;

/// The sessions a human typed into since their last declaration: the Enter rule
/// lifts a wait only after at least one typed byte. And who holds each
/// terminal, so the Run's log keeps one line per take-over and per turn of
/// speech, never one per keystroke.
#[derive(Default)]
pub struct TypedInput {
    typed: Mutex<HashSet<String>>,
    floors: Mutex<Floors>,
}

/// Per session: the last pilot the log named, and who has the floor.
#[derive(Default)]
struct Floors {
    pilots: HashMap<String, Actor>,
    speakers: HashMap<String, Actor>,
}

impl TypedInput {
    /// A wait was declared: the Enter rule counts from here, and the next
    /// input takes the floor again.
    pub(crate) fn forget(&self, session: &str) {
        self.typed.lock().unwrap().remove(session);
        self.floors.lock().unwrap().speakers.remove(session);
    }

    /// Text PDO wrote into the session on the user's gesture (« Copy & send to
    /// terminal ») counts as typed: the Enter that follows lifts a declared
    /// wait as if the user had typed the text.
    pub(crate) fn mark(&self, session: &str) {
        self.typed.lock().unwrap().insert(session.to_string());
    }

    /// Record the frame's signal; `true` when it is an Enter that follows a
    /// typed byte (and the count starts over).
    pub(crate) fn enter_after_typing(&self, session: &str, typed: bool, enter: bool) -> bool {
        let mut sessions = self.typed.lock().unwrap();
        if typed {
            sessions.insert(session.to_string());
        }
        enter && sessions.remove(session)
    }

    /// `actor` pilots `session` now; `true` when it was not the last pilot
    /// named, and then nobody has the floor any more.
    pub(crate) fn pilot(&self, session: &str, actor: &Actor) -> bool {
        let mut floors = self.floors.lock().unwrap();
        if floors.pilots.get(session) == Some(actor) {
            return false;
        }
        floors.pilots.insert(session.to_string(), actor.clone());
        floors.speakers.remove(session);
        true
    }

    /// `actor` typed into `session`; `true` when it takes the floor (it did
    /// not have it).
    pub(crate) fn takes_floor(&self, session: &str, actor: &Actor) -> bool {
        let mut floors = self.floors.lock().unwrap();
        if floors.speakers.get(session) == Some(actor) {
            return false;
        }
        floors.speakers.insert(session.to_string(), actor.clone());
        true
    }
}
