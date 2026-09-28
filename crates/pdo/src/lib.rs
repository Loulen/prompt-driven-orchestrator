//! The composition root of PDO (ADR-0079).
//!
//! The one place that knows every context: it opens the database, wires the
//! adapters onto the ports (`contexts/`, and `ports/` for the ones no single
//! context can answer), assembles the router with the `identify` middleware at
//! its head and the embedded SPA behind it, registers the extensions, runs the
//! daemon's background loops, and carries the CLI. A third-party binary
//! depends on this crate by git tag and its `main` calls [`main_with`] with
//! its extensions: the whole `pdo` binary, CLI and daemon. [`serve`] starts the
//! daemon alone (ADR-0081).
//!
//! Only [`main_with`], [`serve`], [`Config`], [`ReleaseChannel`], [`Daemon`] and
//! the re-exported [`extension_api`] are covered by semver. Everything else in
//! the workspace promises nothing.

mod attributes;
#[doc(hidden)]
pub mod cli;
mod contexts;
mod daemon;
mod entry;
mod extensions;
mod identify;
mod ports;
mod serve;
mod spa;

#[doc(hidden)]
pub use daemon::router_layer::RouterLayer;
#[doc(hidden)]
pub use daemon::{serve_with_config, DaemonConfig, DaemonHandle, ExtensionHostHandle};
#[doc(hidden)]
pub use entry::main_from;
pub use entry::main_with;
#[doc(hidden)]
pub use serve::{compose, composed_router_layer};
pub use serve::{serve, Config, Daemon, ReleaseChannel};

/// The extension API this build of PDO implements.
pub use pdo_extension_api as extension_api;
