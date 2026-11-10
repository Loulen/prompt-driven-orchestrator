//! Provisioning: the untracked files of the source repository a worktree
//! receives (ADR-0070 lineage, #7xx): rules per scope, and their resolution.

pub mod plan;
pub mod rules;

pub use plan::resolve;
pub use rules::*;
