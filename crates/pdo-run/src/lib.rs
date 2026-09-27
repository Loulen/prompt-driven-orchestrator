//! The `run` bounded context (ADR-0079).
//!
//! - [`domain`] holds the pure rules: a Run's events, its state and the
//!   projection that folds one into the other; the graph a Run walks (loops,
//!   routing, conditions, inputs) and the scheduler's evaluators over it; what
//!   a Run allows (transitions, the session cap, the completion, retry and
//!   restart verdicts, the reap policy).
//! - [`application`] holds the ports and the use cases: the scheduler (its
//!   interpreter and its drivers), the node lifecycle (spawn, completion,
//!   failure, skip, start, stop, retry, reattach), the Pipeline Manager's
//!   commands, boot recovery and the stale sweep. Every gesture of an actor
//!   asks the Autorisation d'opération; a transition of the scheduler never
//!   does (D10).
//! - [`adapters`] holds the event store over SQLite, the routes (a Run's
//!   detail and events, `/ws`, the node routes, `/runs/{id}/commands`) and the
//!   agent CLI (`pdo complete`, `fail`, `skip`, `wait-user`, `wait`).
//!
//! The Run's other surfaces (create, list, archive, review, the manager) still
//! live in the legacy crate and call this one. It depends on the kernel and on
//! the pipeline, session, sandbox, workspace, skill and harness contexts.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::runs::{Runs, RunsOptions, RunsPorts};
