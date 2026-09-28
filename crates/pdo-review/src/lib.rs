//! The `review` bounded context (ADR-0079): the review of a Run.
//!
//! - [`domain`] holds the pure rules: a Run's diff as data and the parser of
//!   the unified patch; the review comments' ids, excerpt, author, filter, the
//!   re-map of an anchor, what a reply or a decision changes, the one message
//!   a batch hands the Pipeline Manager.
//! - [`application`] holds the ports (the Run, git, the instance setting) and
//!   the use cases: the raw and the structured diff, the list, the send, the
//!   reply, the resolution and the reopening. Every gesture asks the
//!   Autorisation d'opération.
//! - [`adapters`] holds the routes, the ports over the run context and the
//!   workspace's git, the environment tier of the setting, and the CLI (`pdo
//!   review list`, `pdo review reply`).
//!
//! The comments themselves are Run events: their projection is the run
//! context's (`pdo_run::domain::review_comment`). This context depends on the
//! kernel and on the run, session and workspace contexts.

pub mod adapters;
pub mod application;
pub mod domain;

pub use adapters::http::router;
pub use application::review::{Review, ReviewPorts};
