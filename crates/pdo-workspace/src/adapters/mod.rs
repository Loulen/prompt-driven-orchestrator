//! The workspace's adapters: HTTP on the driving side; SQLite, the filesystem
//! and git on the driven side.

pub mod fs;
pub mod git;
pub mod http;
pub mod sqlite;
