//! The run use cases, over the context's ports.
//!
//! - Reading and writing the log: [`append_event`] (a transition, which never
//!   asks the Autorisation d'opération, D10), [`events`], [`read_run`],
//!   [`projections`], [`run_id`].
//! - The scheduler ([`scheduler`]): the interpreter of the evaluators'
//!   actions and its drivers (advance, node completion, the retry of waiting
//!   nodes, the run-level stall). A transition: it never asks the
//!   authorization.
//! - The node lifecycle ([`node`]): spawn, the primitives, prompt, inputs and
//!   outputs, reap, reattach.
//! - The gestures of an actor, one use case each, each asking the
//!   authorization: [`complete_node`], [`release_completion`], [`fail_node`],
//!   [`skip_node`], [`start_node`], [`stop_node`], [`retry_node`],
//!   [`children_wait`], [`node_diff`] and the Pipeline Manager's
//!   [`commands`].
//! - The daemon's own transitions: [`boot_recovery`], the [`stale`] sweep,
//!   the sandbox preparation ([`sandbox_prep`]).

pub mod admission_count;
pub mod append_event;
pub mod boot_recovery;
pub mod children_wait;
pub mod commands;
pub mod complete_node;
pub mod events;
pub mod fail_node;
pub mod merge_resolver;
pub mod node;
pub mod node_diff;
pub mod operations;
pub mod paths;
pub mod ports;
pub mod projections;
pub mod read_run;
pub mod release_completion;
pub mod reopen;
pub mod retry_node;
pub mod run_id;
pub mod runs;
pub mod sandbox;
pub mod sandbox_prep;
pub mod scheduler;
pub mod session_claim;
pub mod settings;
pub mod skip_node;
pub mod stale;
pub mod start_node;
pub mod stop_node;
pub mod supervision;
