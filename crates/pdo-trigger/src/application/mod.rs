//! The use cases of the trigger context, one per file, and the ports they need
//! (`ports.rs`). Every mutation — and every fire, cron or "Run now" — builds an
//! Operation on the Trigger and goes through the Autorisation d'opération
//! before it touches anything. A cron fire's Acteur is the Trigger itself.
//! The Operation on a Trigger is in the Projets of its target repository
//! (`read_access.rs`), and so are its reads.

pub mod create_trigger;
pub mod delete_trigger;
pub mod fire;
mod guard_for;
pub mod pause_triggers;
pub mod ports;
pub mod read;
pub mod read_access;
pub mod suspension;
pub mod test_guard;
pub mod triggers;
pub mod update_trigger;

use pdo_kernel::authorization::Denied;
use pdo_kernel::{Operation, ResourceKind, Verb};

/// Why a use case of the context did not go through. Each variant maps to one
/// status; its message is the body's `error`, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerError {
    /// An extension refused the Operation (`403 operation_refused`).
    Refused(Denied),
    /// The request is invalid (`400`).
    Invalid(String),
    /// No such Trigger (`404`).
    NotFound(String),
    /// The Trigger cannot do this in its current state (`409`).
    Conflict(String),
    /// The storage failed (`500`); the message says what failed.
    Storage(String),
}

impl From<Denied> for TriggerError {
    fn from(denied: Denied) -> Self {
        TriggerError::Refused(denied)
    }
}

/// The Operation of a gesture on the Triggers: `trigger` × `verb`, aimed at
/// `id` when it names one.
pub(crate) fn operation(verb: Verb, id: Option<&str>) -> Operation {
    let operation = Operation::new(ResourceKind::Trigger, verb);
    match id {
        Some(id) => operation.on(id),
        None => operation,
    }
}

/// The named action of a fire (`trigger` × `fire`).
pub const FIRE: &str = "fire";
/// The named action of the global pause (`trigger` × `pause`).
pub const PAUSE: &str = "pause";
/// The named action of a guard dry-run (`trigger` × `test_guard`): it runs an
/// arbitrary command in a repository.
pub const TEST_GUARD: &str = "test_guard";
