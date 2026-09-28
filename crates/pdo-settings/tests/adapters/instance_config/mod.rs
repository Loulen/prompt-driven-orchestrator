//! The stored tier over SQLite (the legacy crate's `instance_config` tests,
//! moved).

mod knobs;
mod migrations;
mod pause_and_retired;

use sqlx::SqlitePool;

/// A fresh in-memory database with the context's tables.
pub(super) async fn test_db() -> SqlitePool {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    pdo_settings::adapters::sqlite::init(&db).await.unwrap();
    db
}
