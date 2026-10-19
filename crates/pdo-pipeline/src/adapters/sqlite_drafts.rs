//! The drafts over SQLite (ADR-0088): one `pipeline_draft` row per pipeline
//! someone opened, its replicated document encoded, so it survives a restart
//! of the daemon.

use std::collections::HashMap;

use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, DraftStore, StoredDraft};
use crate::domain::draft::DraftContent;

/// Create the table. Idempotent.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_draft (
            pipeline_id TEXT PRIMARY KEY,
            generation  TEXT NOT NULL,
            state       BLOB NOT NULL,
            baseline    TEXT NOT NULL,
            changes     INTEGER NOT NULL,
            updated_at  TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

pub struct SqliteDrafts {
    db: SqlitePool,
}

impl SqliteDrafts {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    async fn read(&self, id: &str) -> Result<Option<StoredDraft>, String> {
        let row = sqlx::query(
            "SELECT generation, state, baseline, changes FROM pipeline_draft WHERE pipeline_id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        let Some(row) = row else { return Ok(None) };
        let baseline: String = row.get("baseline");
        let baseline: DraftContent = serde_json::from_str(&baseline).map_err(|e| e.to_string())?;
        let changes: i64 = row.get("changes");
        Ok(Some(StoredDraft {
            generation: row.get("generation"),
            state: row.get("state"),
            baseline,
            changes: changes.max(0) as u32,
        }))
    }

    async fn write(&self, id: &str, draft: &StoredDraft) -> Result<(), String> {
        let baseline = serde_json::to_string(&draft.baseline).map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO pipeline_draft (pipeline_id, generation, state, baseline, changes, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(pipeline_id) DO UPDATE SET generation = excluded.generation,
               state = excluded.state, baseline = excluded.baseline,
               changes = excluded.changes, updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(&draft.generation)
        .bind(&draft.state)
        .bind(baseline)
        .bind(i64::from(draft.changes))
        .bind(pdo_kernel::clock::now_iso())
        .execute(&self.db)
        .await
        .map(drop)
        .map_err(|e| e.to_string())
    }
}

impl DraftStore for SqliteDrafts {
    fn load<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<StoredDraft>, String>> {
        Box::pin(self.read(id))
    }

    fn save<'a>(
        &'a self,
        id: &'a str,
        draft: &'a StoredDraft,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.write(id, draft))
    }

    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            sqlx::query("DELETE FROM pipeline_draft WHERE pipeline_id = ?")
                .bind(id)
                .execute(&self.db)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        })
    }

    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let mut tx = self.db.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM pipeline_draft WHERE pipeline_id = ?")
                .bind(to)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("UPDATE pipeline_draft SET pipeline_id = ? WHERE pipeline_id = ?")
                .bind(to)
                .bind(from)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())
        })
    }

    fn changes(&self) -> BoxFuture<'_, Result<HashMap<String, u32>, String>> {
        Box::pin(async move {
            let rows = sqlx::query("SELECT pipeline_id, changes FROM pipeline_draft")
                .fetch_all(&self.db)
                .await
                .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let changes: i64 = row.get("changes");
                    (row.get("pipeline_id"), changes.max(0) as u32)
                })
                .collect())
        })
    }
}
