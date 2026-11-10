//! The adapters of the trigger context: the driving side (the routes, the
//! heartbeat, the `pdo trigger` command line) and the driven side (SQLite,
//! `sh`, and the other contexts).

pub mod cli;
pub mod contexts;
pub mod env;
pub mod guard_shell;
pub mod heartbeat;
pub mod http;
pub mod instance_defaults;
pub mod sqlite;
