//! The skill context's adapters: HTTP on the driving side; SQLite, the
//! filesystem, the Source scans and the delivery into a worktree on the driven
//! side, and its word on a Projet's skills selection (`projects`). Every git
//! call goes through the workspace context's git adapter.

pub mod delivery;
pub mod fs;
pub mod http;
pub mod projects;
pub mod sidecar_zip;
pub mod source;
pub mod sqlite;
