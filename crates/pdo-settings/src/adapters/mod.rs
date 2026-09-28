//! The adapters of the settings context: the driving side (the routes) and the
//! driven side (SQLite, the sections of the page each context discloses, and
//! the other contexts, both ways).

pub mod contexts;
pub mod http;
pub mod sections;
pub mod sqlite;
