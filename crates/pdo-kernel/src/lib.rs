//! PDO's shared kernel (DDD *shared kernel*, ADR-0079).
//!
//! What every bounded context shares, and nothing else: the clock, the
//! [`Actor`] and the [`Operation`] (defined by the extension API and re-exported
//! here), the neutral authorization point, the event bus, the extension host,
//! and the names of the environment variables the instance configuration reads.
//! Keep it small and stable: a concept that belongs to one context lives in that
//! context's crate.

pub mod actor;
pub mod authorization;
pub mod clock;
pub mod env;
pub mod event_bus;
pub mod extension_host;
pub mod operation;

pub use actor::Actor;
pub use operation::{Operation, ResourceKind, Verb};
