//! The workspace's adapters: HTTP on the driving side; SQLite, the filesystem
//! and git on the driven side; the disclosure of its instance default.

pub mod fs;
pub mod git;
pub mod http;
pub mod instance_defaults;
pub mod sqlite;
