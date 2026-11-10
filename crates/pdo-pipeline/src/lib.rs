//! The `pipeline` bounded context (ADR-0079): the pipeline document and
//! everything that edits, moves or reads it as a document.
//!
//! - [`domain`] holds the pure rules: the model and its parse, the
//!   references a pipeline makes to itself, its loop regions and cycles, the
//!   migrator, the portable document, the Claude Code workflow import, the
//!   node library entry, a pipeline's identity (name ⇄ stem), and what a Run
//!   in flight forbids an edit to touch.
//! - [`application`] holds the ports and the use cases; every mutation goes
//!   through the Autorisation d'opération of the kernel.
//! - [`adapters`] holds the HTTP routes, the instance registry on disk, the
//!   node library on disk, the registry's watcher, and the bank of the skill
//!   context behind the portable document.
//!
//! It depends on the kernel and on the skill, harness and workspace contexts.

pub mod adapters;
pub mod application;
pub mod domain;

pub use application::{Pipelines, PipelinesPorts};
