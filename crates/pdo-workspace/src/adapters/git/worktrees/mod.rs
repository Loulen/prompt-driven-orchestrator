//! The worktree lifecycle of a Run: create, classify, merge back, deliver,
//! reap (ADR-0009, ADR-0036, ADR-0037, ADR-0060).
//!
//! Plain `&Path` / `&str` / `i64` in, a shell-out to `git` or `std::fs` out: no
//! Run state, no event log, no tmux. `MergeResult` (the git *effect*) lives
//! here; it is deliberately distinct from the Run's merge *decision*.
//!
//! The Run context calls these functions until it migrates onto the `Git`
//! port of this crate.

mod checks;
mod classify;
mod delivery;
mod ensure;
mod merge;
mod reap;
mod registry;

pub use checks::*;
pub use classify::*;
pub use delivery::*;
pub use ensure::*;
pub use merge::*;
pub use reap::*;
pub use registry::*;

pub use crate::domain::commit_identity::CommitIdentity;
pub use crate::domain::worktree_layout::*;
