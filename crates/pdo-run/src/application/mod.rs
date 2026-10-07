//! The run use cases, over the context's ports.
//!
//! - Reading and writing the log: [`append_event`] (a transition, which never
//!   asks the Autorisation d'opération, D10), [`events`], [`read_run`],
//!   [`projections`], [`run_id`]; the Résumés de Run the lists read, rebuilt
//!   from the log when they are not its own ([`run_summary`], ADR-0084).
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
//! - The Run's surfaces: creating a Run ([`create_run`]), forgetting one
//!   ([`forget_run`]), editing its repositories ([`edit_repos`]) and its own
//!   pipeline ([`run_pipeline`]), starting and stopping its Pipeline Manager
//!   ([`manager`]) — each a gesture that asks the authorization — and the
//!   reads: the list, the reapable Runs and the children ([`list`]), an
//!   artifact ([`artifact`]), the refs, a file at a ref and the source drift
//!   ([`refs`]). Each read asks the authorization too ([`read_access`],
//!   CORE-5): a list keeps the Runs the actor may read.
//! - What a command triggers once it was authorized: the archive of a Run and
//!   its descendants ([`archive`]), `retry_all` ([`retry_all`]).
//! - The daemon's own transitions: [`boot_recovery`], the [`stale`] sweep,
//!   the sandbox preparation ([`sandbox_prep`]), an edit of a run-scoped
//!   pipeline on disk ([`pipeline_modified`]).

pub mod admission_count;
pub mod append_event;
pub mod archive;
pub mod artifact;
pub mod boot_recovery;
pub mod children_wait;
pub mod commands;
pub mod complete_node;
pub mod create_run;
pub mod edit_repos;
pub mod events;
pub mod fail_node;
pub mod forget_run;
pub mod list;
pub mod manager;
pub mod node;
pub mod node_diff;
pub mod operations;
pub mod paths;
pub mod pipeline_modified;
pub mod ports;
pub mod projections;
pub mod read_access;
pub mod read_run;
pub mod refs;
pub mod release_completion;
pub mod reopen;
pub mod retry_all;
pub mod retry_node;
pub mod run_id;
pub mod run_pipeline;
pub mod run_summary;
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
