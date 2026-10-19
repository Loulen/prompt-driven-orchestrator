//! The `telemetry` bounded context (ADR-0079, ADR-0090): the instance's
//! metrics and traces, pushed over OTLP to the organisation's collector, its
//! metrics also read on `/metrics` in the Prometheus format.
//!
//! - [`domain`] holds the pure rules: the setting (each channel off by
//!   default) and its secret headers, the attributes a measure carries and how
//!   a repository is named (its remote, never a path), the Run lifecycle read
//!   off the events. The Actor is an attribute only with « include the
//!   Actor », off by default.
//! - [`application`] holds the hub, which rebuilds its providers when the
//!   channels change (no restart), the instruments, the recorder of the Runs
//!   and the status of the last export.
//! - [`adapters`] holds the OTLP exporters over HTTP, the `/metrics` reader,
//!   the routes (status, test export, `/metrics`), the HTTP layer that
//!   measures every request, the environment tier of the setting and its
//!   disclosure on the settings page, git.
//!
//! It depends on the kernel and on the run context's events.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::hub::{Identity, Telemetry};
