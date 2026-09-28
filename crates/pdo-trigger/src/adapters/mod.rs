//! The adapters of the trigger context: the driving side (the routes, the
//! heartbeat) and the driven side (SQLite, `sh`, and the other contexts).

pub mod contexts;
pub mod env;
pub mod guard_shell;
pub mod heartbeat;
pub mod http;
pub mod sqlite;
