//! A context's domain as the architecture test expects it: pure rules only.
//!
//! Mentioning sqlx, axum or `tokio::process` in a comment is fine; so is a
//! string that happens to contain `adapters::`.

mod rules;

pub use rules::next_status;

pub const LABEL: &str = "adapters::are not imported here";
