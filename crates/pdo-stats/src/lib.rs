//! The `stats` bounded context (ADR-0079).
//!
//! - [`domain`] holds the pure rules: the Overview, Cost and Performance folds,
//!   the R-7 median and the Tukey fences, the absorptions (their keys, their
//!   resolution order, their frozen scope) and the price sync's normalisation.
//! - [`application`] holds the ports and the use cases: the three reads, the
//!   absorptions' Combine and Uncombine (each asks the Autorisation
//!   d'opération), the rename-born absorption, the price sync (asked too) and
//!   its boot refresh (a transition of the daemon, never asked, D10).
//! - [`adapters`] holds the routes (`/stats/*`, `/settings/cost-prices/sync`),
//!   the absorptions' SQLite store, the reads of the run context's event log
//!   (through its `EventStore` and `EventWindows` ports, never the table), the
//!   Runs' harness files, the price files and the price source.
//!
//! It depends on the kernel and on the run context and the contexts upstream of
//! it (pipeline, sandbox, harness, workspace, platform); the settings context
//! downstream reads it through what it makes `pub`.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::stats::{Stats, StatsOptions, StatsPorts};
