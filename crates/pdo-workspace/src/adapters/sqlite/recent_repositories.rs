//! The repositories Runs worked in, read off the `run_started` events.
//!
//! The event table belongs to the Run context; this adapter only reads the
//! `target_repo` and the Run of each start, newest first, and never writes
//! there.

use sqlx::SqlitePool;

use crate::application::ports::{
    BoxFuture, RecentRepositories, RecentRepository, StorageError, StoreResult,
};

/// The [`RecentRepositories`] over the daemon's event log.
#[derive(Clone)]
pub struct SqliteRecentRepositories {
    db: SqlitePool,
}

impl SqliteRecentRepositories {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl RecentRepositories for SqliteRecentRepositories {
    fn recent(&self) -> BoxFuture<'_, StoreResult<Vec<RecentRepository>>> {
        Box::pin(async move {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT run_id, payload FROM events \
                 WHERE kind = 'run_started' AND payload IS NOT NULL \
                 ORDER BY ts DESC",
            )
            .fetch_all(&self.db)
            .await
            .map_err(|e| StorageError(e.to_string()))?;

            let mut seen = std::collections::HashSet::new();
            let mut repos = Vec::new();
            for (run_id, payload) in &rows {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                    continue;
                };
                if let Some(repo) = value["target_repo"].as_str() {
                    if seen.insert(repo.to_string()) {
                        repos.push(RecentRepository::new(repo, run_id));
                    }
                }
            }
            Ok(repos)
        })
    }
}
