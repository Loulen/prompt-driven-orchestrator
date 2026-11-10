//! The drafts over SQLite (ADR-0088): one `pipeline_draft` row per pipeline
//! someone opened, its replicated document encoded, so it survives a restart
//! of the daemon.

use std::collections::HashMap;

use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, DraftStore, StoredDraft};
use crate::domain::draft::{DraftContent, DraftSummary};

/// Create the table. Idempotent.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_draft (
            pipeline_id TEXT PRIMARY KEY,
            generation  TEXT NOT NULL,
            state       BLOB NOT NULL,
            baseline    TEXT NOT NULL,
            changes     INTEGER NOT NULL,
            nodes       INTEGER NOT NULL DEFAULT 0,
            updated_at  TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    // A table from before `nodes`: the column joins it.
    let has_nodes: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('pipeline_draft') WHERE name = 'nodes'",
    )
    .fetch_one(db)
    .await?;
    if !has_nodes {
        sqlx::query("ALTER TABLE pipeline_draft ADD COLUMN nodes INTEGER NOT NULL DEFAULT 0")
            .execute(db)
            .await?;
    }
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
            "SELECT generation, state, baseline, changes, nodes FROM pipeline_draft WHERE pipeline_id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        let Some(row) = row else { return Ok(None) };
        let baseline: String = row.get("baseline");
        let baseline: DraftContent = serde_json::from_str(&baseline).map_err(|e| e.to_string())?;
        Ok(Some(StoredDraft {
            generation: row.get("generation"),
            state: row.get("state"),
            baseline,
            changes: count(&row, "changes"),
            nodes: count(&row, "nodes"),
        }))
    }

    async fn write(&self, id: &str, draft: &StoredDraft) -> Result<(), String> {
        let baseline = serde_json::to_string(&draft.baseline).map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO pipeline_draft (pipeline_id, generation, state, baseline, changes, nodes, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(pipeline_id) DO UPDATE SET generation = excluded.generation,
               state = excluded.state, baseline = excluded.baseline,
               changes = excluded.changes, nodes = excluded.nodes,
               updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(&draft.generation)
        .bind(&draft.state)
        .bind(baseline)
        .bind(i64::from(draft.changes))
        .bind(i64::from(draft.nodes))
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

    fn summaries(&self) -> BoxFuture<'_, Result<HashMap<String, DraftSummary>, String>> {
        Box::pin(async move {
            let rows = sqlx::query("SELECT pipeline_id, changes, nodes FROM pipeline_draft")
                .fetch_all(&self.db)
                .await
                .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let summary = DraftSummary {
                        changes: count(&row, "changes"),
                        nodes: count(&row, "nodes"),
                        participants: Vec::new(),
                    };
                    (row.get("pipeline_id"), summary)
                })
                .collect())
        })
    }
}

fn count(row: &sqlx::sqlite::SqliteRow, column: &str) -> u32 {
    let value: i64 = row.get(column);
    value.max(0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(changes: u32, nodes: u32) -> StoredDraft {
        StoredDraft {
            generation: "g1".into(),
            state: vec![1, 2, 3],
            baseline: DraftContent::new(serde_json::json!({"name": "p"}), []),
            changes,
            nodes,
        }
    }

    #[tokio::test]
    async fn keeps_a_draft_and_lists_its_changes_and_nodes() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        let store = SqliteDrafts::new(db);
        store.save("p", &draft(2, 5)).await.unwrap();
        let kept = store.load("p").await.unwrap().unwrap();
        assert_eq!(
            (kept.changes, kept.nodes, kept.state),
            (2, 5, vec![1, 2, 3])
        );
        let listed = store.summaries().await.unwrap();
        assert_eq!((listed["p"].changes, listed["p"].nodes), (2, 5));
    }

    #[tokio::test]
    async fn a_table_from_before_the_node_count_takes_it() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE pipeline_draft (
                pipeline_id TEXT PRIMARY KEY,
                generation  TEXT NOT NULL,
                state       BLOB NOT NULL,
                baseline    TEXT NOT NULL,
                changes     INTEGER NOT NULL,
                updated_at  TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pipeline_draft VALUES ('old', 'g0', x'00', '{\"pipeline\":{}}', 1, '')",
        )
        .execute(&db)
        .await
        .unwrap();
        init(&db).await.unwrap();
        let store = SqliteDrafts::new(db);
        assert_eq!(store.load("old").await.unwrap().unwrap().nodes, 0);
        store.save("p", &draft(1, 3)).await.unwrap();
        assert_eq!(store.summaries().await.unwrap()["p"].nodes, 3);
    }
}
