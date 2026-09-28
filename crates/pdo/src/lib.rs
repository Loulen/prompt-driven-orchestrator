//! The composition root of PDO (ADR-0079).
//!
//! The one place that knows every context: it wires the adapters onto the
//! ports, assembles the router with the `identify` middleware at its head,
//! registers the extensions and carries the CLI. A third-party binary depends
//! on this crate by git tag and calls [`serve`] with its extensions (ADR-0081).
//!
//! Only [`serve`], [`Config`], [`Daemon`] and the re-exported
//! [`extension_api`] are covered by semver. Everything else in the workspace
//! promises nothing.
//!
//! During the migration, the bounded contexts still live in the legacy crate
//! (`pdo_daemon`), which [`serve`] delegates to.

mod attributes;
mod contexts;
mod extensions;
mod identify;
mod serve;

#[doc(hidden)]
pub use serve::{compose, composed_router_layer};
pub use serve::{serve, Config, Daemon};

/// The extension API this build of PDO implements.
pub use pdo_extension_api as extension_api;
