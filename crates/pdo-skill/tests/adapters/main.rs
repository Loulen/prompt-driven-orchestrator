//! Stage 3 of the test pyramid (ADR-0080): the skill context's adapters
//! against the real resource — SQLite, a real filesystem, temporary git
//! repositories and worktrees.
//!
//! One binary for every adapter, so the crate links its tests once.

mod common;
mod delivery;
mod fs_content;
mod source_scanner;
mod sqlite_index;
