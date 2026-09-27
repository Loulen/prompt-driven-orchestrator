//! The event log over SQLite: the `events` table and the tombstones of the
//! forgotten Runs (ADR-0024).
//!
//! The free functions are what the legacy crate still calls directly, until
//! the code that calls them moves onto the [`EventStore`] port.

use anyhow::{Context, Result};

use crate::application::ports::{BoxFuture, EventStore};
use crate::domain::event::{Event, EventKind};

/// Create the log's tables and indexes. Idempotent.
pub async fn init(db: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,
            node_id TEXT,
            iter INTEGER,
            payload JSON
        )",
    )
    .execute(db)
    .await
    .context("failed to create events table")?;

    // `GET /stats/overview` runs `WHERE kind = ? AND ts >= ? AND ts < ? GROUP BY
    // strftime(...)`; the composite `(kind, ts)` is leftmost-prefix optimal for it.
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_kind_ts ON events(kind, ts)")
        .execute(db)
        .await
        .context("failed to create idx_events_kind_ts")?;
    // After the indexed `run_started` cohort lookup, Sessions joins only each
    // selected Run's `node_started` rows in append order.
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_run_kind_id ON events(run_id, kind, id)")
        .execute(db)
        .await
        .context("failed to create idx_events_run_kind_id")?;

    // Tombstones for forgotten runs (ADR-0024): a run_id present here can never
    // receive events again — the guard is in `append`.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS forgotten_runs (
            run_id TEXT PRIMARY KEY,
            forgotten_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await
    .context("failed to create forgotten_runs table")?;
    Ok(())
}

/// Insert `event`. Refuses a tombstoned run in the SAME statement as the
/// insert, or a concurrent forget opens a TOCTOU window (ADR-0024).
pub async fn append(db: &sqlx::SqlitePool, event: &Event) -> Result<()> {
    let kind_str = serde_json::to_value(&event.kind)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let payload_str = event.payload.as_ref().map(|p| p.to_string());

    let result = sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload)
         SELECT ?, ?, ?, ?, ?, ?
         WHERE NOT EXISTS (SELECT 1 FROM forgotten_runs WHERE run_id = ?)",
    )
    .bind(&event.run_id)
    .bind(&event.ts)
    .bind(&kind_str)
    .bind(&event.node_id)
    .bind(event.iter)
    .bind(&payload_str)
    .bind(&event.run_id)
    .execute(db)
    .await
    .context("failed to append event")?;
    if result.rows_affected() == 0 {
        anyhow::bail!("run {} has been forgotten", event.run_id);
    }
    Ok(())
}

/// Every event of `run_id`, in append order.
pub async fn load(db: &sqlx::SqlitePool, run_id: &str) -> Result<Vec<Event>> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT id, run_id, ts, kind, node_id, iter, payload FROM events WHERE run_id = ? ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
    .context("failed to load events")?;

    Ok(rows.into_iter().map(|r| r.into_event()).collect())
}

/// The id of every Run of the log, newest first.
pub async fn run_ids(db: &sqlx::SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT run_id FROM events ORDER BY run_id DESC")
            .fetch_all(db)
            .await
            .context("failed to load run ids")?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Whether a run_id has been tombstoned by forget (ADR-0024).
pub async fn is_forgotten(db: &sqlx::SqlitePool, run_id: &str) -> Result<bool> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM forgotten_runs WHERE run_id = ? LIMIT 1")
            .bind(run_id)
            .fetch_optional(db)
            .await
            .context("failed to query forgotten_runs")?;
    Ok(row.is_some())
}

/// Whether a Run was started with `run_id` as its parent; `true` when the
/// query fails.
pub async fn has_children(db: &sqlx::SqlitePool, run_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM events WHERE kind = 'run_started' \
         AND json_extract(payload, '$.parent_run_id') = ?",
    )
    .bind(run_id)
    .fetch_one(db)
    .await
    .map(|n| n > 0)
    .unwrap_or(true)
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    run_id: String,
    ts: String,
    kind: String,
    node_id: Option<String>,
    iter: Option<i64>,
    payload: Option<String>,
}

impl EventRow {
    fn into_event(self) -> Event {
        let kind: EventKind = serde_json::from_value(serde_json::Value::String(self.kind.clone()))
            .unwrap_or(EventKind::RunStarted);

        let payload = self
            .payload
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        Event {
            id: Some(self.id),
            run_id: self.run_id,
            ts: self.ts,
            kind,
            node_id: self.node_id,
            iter: self.iter,
            payload,
        }
    }
}

/// The [`EventStore`] over a pool of `pdo.db`.
pub struct SqliteEventStore {
    db: sqlx::SqlitePool,
}

impl SqliteEventStore {
    pub fn new(db: sqlx::SqlitePool) -> Self {
        Self { db }
    }
}

impl EventStore for SqliteEventStore {
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<()>> {
        Box::pin(append(&self.db, event))
    }

    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<Vec<Event>>> {
        Box::pin(load(&self.db, run_id))
    }

    fn run_ids(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(run_ids(&self.db))
    }

    fn is_forgotten<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(is_forgotten(&self.db, run_id))
    }

    fn has_children<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(has_children(&self.db, run_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> SqliteEventStore {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init(&db).await.unwrap();
        SqliteEventStore::new(db)
    }

    fn event(run_id: &str, kind: EventKind, payload: Option<serde_json::Value>) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: "2026-09-27T12:00:00.000Z".into(),
            kind,
            node_id: Some("worker".into()),
            iter: Some(2),
            payload,
        }
    }

    #[tokio::test]
    async fn an_appended_event_reads_back_whole_with_its_id_in_append_order() {
        let store = store().await;
        let first = event(
            "r1",
            EventKind::RunStarted,
            Some(serde_json::json!({ "pipeline_name": "p" })),
        );
        store.append(&first).await.unwrap();
        store
            .append(&event("r1", EventKind::NodeStarted, None))
            .await
            .unwrap();
        store
            .append(&event("r2", EventKind::RunStarted, None))
            .await
            .unwrap();

        let events = store.events("r1").await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, Some(1));
        assert_eq!(events[0].kind, EventKind::RunStarted);
        assert_eq!(events[0].node_id.as_deref(), Some("worker"));
        assert_eq!(events[0].iter, Some(2));
        assert_eq!(events[0].payload, first.payload);
        assert_eq!(events[1].kind, EventKind::NodeStarted);
        assert_eq!(events[1].payload, None);
        assert!(store.events("unknown").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_run_ids_come_newest_first_once_each() {
        let store = store().await;
        for run_id in ["20260101-a", "20260301-c", "20260201-b", "20260301-c"] {
            store
                .append(&event(run_id, EventKind::RunStarted, None))
                .await
                .unwrap();
        }
        assert_eq!(
            store.run_ids().await.unwrap(),
            vec!["20260301-c", "20260201-b", "20260101-a"]
        );
    }

    #[tokio::test]
    async fn a_forgotten_run_refuses_every_append() {
        let store = store().await;
        sqlx::query("INSERT INTO forgotten_runs (run_id, forgotten_at) VALUES ('gone', 'now')")
            .execute(&store.db)
            .await
            .unwrap();
        let error = store
            .append(&event("gone", EventKind::RunStarted, None))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "run gone has been forgotten");
        assert!(store.events("gone").await.unwrap().is_empty());
        assert!(is_forgotten(&store.db, "gone").await.unwrap());
        assert!(!is_forgotten(&store.db, "alive").await.unwrap());
        // The same answer through the port the use cases read it by.
        let port: &dyn EventStore = &store;
        assert!(port.is_forgotten("gone").await.unwrap());
        assert!(!port.is_forgotten("alive").await.unwrap());
    }

    #[tokio::test]
    async fn a_run_has_children_once_one_names_it_as_its_parent() {
        let store = store().await;
        assert!(!store.has_children("parent").await);
        store
            .append(&event(
                "child",
                EventKind::RunStarted,
                Some(serde_json::json!({ "parent_run_id": "parent" })),
            ))
            .await
            .unwrap();
        assert!(store.has_children("parent").await);
        assert!(!store.has_children("child").await);
    }

    #[tokio::test]
    async fn an_unknown_kind_reads_back_as_run_started() {
        let store = store().await;
        sqlx::query("INSERT INTO events (run_id, ts, kind) VALUES ('r', 't', 'from_the_future')")
            .execute(&store.db)
            .await
            .unwrap();
        assert_eq!(
            store.events("r").await.unwrap()[0].kind,
            EventKind::RunStarted
        );
    }

    #[tokio::test]
    async fn init_is_idempotent() {
        let store = store().await;
        init(&store.db).await.unwrap();
    }
}
