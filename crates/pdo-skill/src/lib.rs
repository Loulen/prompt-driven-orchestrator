//! The `skill` bounded context (ADR-0079): the Banque de skills, its import
//! from a Source, the skills selected per tier and their delivery into the
//! worktree a node works in.
//!
//! - [`domain`] holds the pure rules: the `SKILL.md` gate, file paths, the
//!   seeded skills, the additive resolution of the tiers, the reading of a
//!   Source, the collision and update rules of an import, the exclusion lines.
//! - [`application`] holds the ports and the use cases; every mutation goes
//!   through the Autorisation d'opération of the kernel.
//! - [`adapters`] holds the HTTP routes, the SQLite index, the content on disk,
//!   the Source scans and the delivery into a worktree. Every git call goes
//!   through the workspace context's git adapter.
//!
//! It depends on the kernel and on the workspace context only.

pub mod adapters;
pub mod application;
pub mod domain;

pub use application::{Skills, SkillsPorts};
