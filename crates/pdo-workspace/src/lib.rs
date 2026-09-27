//! The `workspace` bounded context (ADR-0079): Projets, the repositories a Run
//! works in, provisioning rules, the filesystem explorer, and git.
//!
//! - [`domain`] holds the pure rules (membership, provisioning resolution,
//!   branch freshness, fast-forward conditions).
//! - [`application`] holds the ports and the use cases; every mutation goes
//!   through the Autorisation d'opération of the kernel.
//! - [`adapters`] holds the HTTP routes, the SQLite stores, the filesystem, and
//!   `git` — every git call of this context goes through [`adapters::git`].
//!
//! It depends on the kernel only. The Projet carries settings that belong to
//! contexts downstream of it (the agentic profile, the skills selection) as
//! opaque stored JSON: see [`domain::project::StoredSetting`].

pub mod adapters;
pub mod application;
pub mod domain;
