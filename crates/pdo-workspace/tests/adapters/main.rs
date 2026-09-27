//! Stage 3 of the test pyramid (ADR-0080): the workspace's adapters against
//! the real resource — temporary git repositories, a real filesystem, SQLite.
//!
//! One binary for every adapter, so the crate links its tests once.

mod common;
mod fs_browse;
mod fs_provisioner;
mod git_branches;
mod git_clone;
mod git_fast_forward;
mod git_fetch;
mod git_repository;
mod git_scaffold;
mod git_worktrees;
mod sqlite_projects;
mod sqlite_provisioning_rules;
mod sqlite_recent;
