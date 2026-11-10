//! PDO's shared kernel (DDD *shared kernel*, ADR-0079).
//!
//! What every bounded context shares, and nothing else: the clock, the
//! [`Actor`] and the [`Operation`] (defined by the extension API and re-exported
//! here), the neutral authorization point, the event bus, the extension host
//! (with the checks an extension goes through at registration, the delivery of
//! the events, the `OperationApplied` record, the entity attributes), the shape an
//! instance default is disclosed with, the names of the environment
//! variables the instance defaults read, the Public URL rule, how a repository path
//! matches a Projet's member, the names of the Projets the extensions read, and the
//! count of the daemon's open WebSockets.
//! Keep it small and stable: a concept that belongs to one context lives in that
//! context's crate.

pub mod actor;
pub mod applied;
pub mod attributes;
pub mod authorization;
pub mod clock;
mod delivery;
pub mod env;
pub mod event_bus;
pub mod extension_host;
pub mod instance_default;
pub mod live_connections;
pub mod operation;
pub mod project_names;
pub mod public_url;
pub mod registration;
pub mod repository_path;

pub use actor::Actor;
pub use operation::{Operation, ResourceKind, Verb};
