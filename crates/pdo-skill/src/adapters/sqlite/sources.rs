//! The sources the operator scanned, for the import popup's "Recent sources".

use sqlx::{Row, SqlitePool};

use super::storage;
use crate::application::ports::{BoxFuture, ScannedSource, SourceHistory, StoreResult};

/// The [`SourceHistory`] port over SQLite.
pub struct SqliteSourceHistory {
    db: SqlitePool,
}

impl SqliteSourceHistory {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl SourceHistory for SqliteSourceHistory {
    fn remember<'a>(&'a self, source: &'a ScannedSource) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO skill_sources (url, git_ref, path, last_used_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(url, git_ref, path) DO UPDATE SET last_used_at = excluded.last_used_at",
            )
            .bind(&source.url)
            .bind(&source.git_ref)
            .bind(&source.path)
            .bind(&source.last_used_at)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn recent(&self, limit: usize) -> BoxFuture<'_, StoreResult<Vec<ScannedSource>>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT url, git_ref, path, last_used_at FROM skill_sources \
                 ORDER BY last_used_at DESC LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(&self.db)
            .await
            .map_err(storage)?;
            Ok(rows
                .iter()
                .map(|row| ScannedSource {
                    url: row.get("url"),
                    git_ref: row.get("git_ref"),
                    path: row.get("path"),
                    last_used_at: row.get("last_used_at"),
                })
                .collect())
        })
    }
}
