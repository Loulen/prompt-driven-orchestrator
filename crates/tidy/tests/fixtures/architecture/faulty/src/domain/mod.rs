//! A domain that reaches for technical concerns: every import below must be
//! reported by the architecture test.

use sqlx::SqlitePool;

use crate::adapters::sqlite::SqliteStore;

pub fn spawn() {
    let _ = tokio::process::Command::new("git");
}

pub fn router() -> axum::Router {
    axum::Router::new()
}

pub fn store(_pool: SqlitePool) -> SqliteStore {
    todo!()
}
