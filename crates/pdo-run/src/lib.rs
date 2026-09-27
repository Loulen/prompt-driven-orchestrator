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
//!   commands, the Run's surfaces (create, forget, archive, the repositories,
//!   the run-scoped pipeline, the manager, the reads of the list), boot
//!   recovery and the stale sweep. Every gesture of an actor
//!   asks the Autorisation d'opération; a transition of the scheduler never
//!   does (D10).
//! - [`adapters`] holds the event store over SQLite, a Run's cost and LOC read
//!   from disk, the price table's files, the routes (the Run's surfaces, a
//!   Run's detail and events, `/ws`, the node routes, `/runs/{id}/commands`)
//!   and the CLI (`pdo complete`, `fail`, `skip`, `wait-user`, `run wait`,
//!   `run create`, `reap`).
//!
//! The review of a Run (its diff, its comments) is the review context's. This
//! one depends on the kernel and on the pipeline, session, sandbox, workspace,
//! skill and harness contexts; the contexts downstream of it (review, trigger,
//! stats, settings) read it through what it makes `pub`: the use cases of
//! [`Runs`], the domain types, [`router`].

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::runs::{Runs, RunsOptions, RunsPorts};
