//! The `trigger` bounded context (ADR-0079): a Trigger is a Run template a cron
//! schedule fires (CONTEXT.md → *Trigger*).
//!
//! - [`domain`] holds the pure rules: a Trigger and its fire history, the cron
//!   schedule, when it fires next, the firing decision and the plan of a tick,
//!   the guard's bounds, what a configuration must satisfy.
//! - [`application`] holds the ports and the use cases: create, edit, delete,
//!   the global pause, the guard's dry-run, "Run now", the heartbeat's tick, the
//!   reads. Every mutation and every fire asks the Autorisation d'opération
//!   (`trigger` × `create` / `update` / `delete` / `pause` / `test_guard` /
//!   `fire`); a cron fire asks with the Acteur `Trigger`.
//! - [`adapters`] holds the Triggers over SQLite, the guard run by `sh`, the
//!   heartbeat (the clock that ticks), what the context reads of the other
//!   contexts (the Runs a fire starts, the pipelines, the repositories, the
//!   staging profiles, the audit journal), and the routes `/triggers*`.
//!
//! It depends on the kernel and on the run, pipeline, workspace, sandbox,
//! skill, harness and platform contexts. The settings context, downstream,
//! implements the [`application::ports::TriggerSettings`] it reads.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::triggers::{Triggers, TriggersOptions, TriggersPorts};
