//! The event log over SQLite: the `events` table and the tombstones of the
//! forgotten Runs (ADR-0024), with the Résumé de Run each append keeps in its
//! own transaction (ADR-0084, `sqlite_run_summaries`).
//!
//! The free functions are what the readers outside a use case call directly
//! (the session context's Run-side ports, the boot's schema), beside the
//! [`EventStore`] port.

use anyhow::{Context, Result};
use sqlx::SqliteConnection;

use super::sqlite_run_summaries as summaries;
use crate::application::ports::{BoxFuture, EventStore};
use crate::domain::event::{Event, EventKind};
use crate::domain::run_summary::RunSummary;

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
    summaries::init(db).await
}

/// Insert `event`, and write its Run's Résumé in the same transaction
/// (ADR-0084). Refuses a tombstoned run in the SAME statement as the insert,
/// or a concurrent forget opens a TOCTOU window (ADR-0024).
pub async fn append(db: &sqlx::SqlitePool, event: &Event) -> Result<()> {
    let kind_str = serde_json::to_value(&event.kind)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let payload_str = event.payload.as_ref().map(|p| p.to_string());

    // The insert comes first: it takes the write lock, so the log the Résumé
    // is computed from is the one the transaction commits.
    let mut tx = db.begin().await?;
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
    .execute(&mut *tx)
    .await
    .context("failed to append event")?;
    if result.rows_affected() == 0 {
        anyhow::bail!("run {} has been forgotten", event.run_id);
    }
    summaries::write_in(&mut tx, &event.run_id, result.last_insert_rowid()).await?;
    tx.commit().await.context("failed to commit the event")?;
    Ok(())
}

/// Every event of `run_id`, in append order.
pub async fn load(db: &sqlx::SqlitePool, run_id: &str) -> Result<Vec<Event>> {
    let mut conn = db.acquire().await.context("failed to load events")?;
    load_in(&mut conn, run_id).await
}

/// [`load`], on `conn` (a transaction's included).
pub(crate) async fn load_in(conn: &mut SqliteConnection, run_id: &str) -> Result<Vec<Event>> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT id, run_id, ts, kind, node_id, iter, payload FROM events WHERE run_id = ? ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(conn)
    .await
    .context("failed to load events")?;

    Ok(rows.into_iter().map(|r| r.into_event()).collect())
}

/// ADR-0087's one migration of the existing Runs: each `run_started` written
/// before the Projet was recorded (no `project_id` key) receives the Projet
/// the rule before ADR-0087 gave it — `legacy_project` of its primary
/// repository (the daemon's repo root when it names none), or of another
/// spelling of it — else `null` (no Projet). Idempotent: a `run_started`
/// carrying the key, `null` included, is never touched again, so a later
/// change of the members moves no Run. How many Runs it recorded.
///
/// Runs at boot, before the Résumés are checked: the caller rebuilds them when
/// it recorded any.
pub async fn record_legacy_projects<F, Fut>(
    db: &sqlx::SqlitePool,
    repo_root: &std::path::Path,
    legacy_project: F,
) -> Result<usize>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>>>,
{
    let starts: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, json_extract(payload, '$.target_repo') FROM events \
         WHERE kind = 'run_started' AND payload IS NOT NULL \
           AND json_type(payload, '$.project_id') IS NULL",
    )
    .fetch_all(db)
    .await
    .context("failed to read the runs without a recorded project")?;
    if starts.is_empty() {
        return Ok(0);
    }
    let mut recorded = Vec::with_capacity(starts.len());
    for (id, target_repo) in &starts {
        let repo = target_repo
            .clone()
            .unwrap_or_else(|| repo_root.to_string_lossy().into_owned());
        let mut project = None;
        for spelling in pdo_kernel::repository_path::member_spellings(&repo) {
            if let Some(found) = legacy_project(spelling).await? {
                project = Some(found);
                break;
            }
        }
        recorded.push((*id, project));
    }
    let mut tx = db.begin().await?;
    for (id, project) in &recorded {
        sqlx::query(
            "UPDATE events SET payload = json_set(payload, '$.project_id', ?) WHERE id = ?",
        )
        .bind(project)
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("failed to record the project of a run")?;
    }
    tx.commit()
        .await
        .context("failed to commit the projects of the runs")?;
    Ok(recorded.len())
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

/// Forget `run_id` (ADR-0024): tombstone it and purge its events in ONE
/// transaction, so a late writer never observes "events gone, no tombstone yet"
/// and resurrects the Run.
pub async fn forget(db: &sqlx::SqlitePool, run_id: &str) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO forgotten_runs (run_id, forgotten_at) VALUES (?, ?)")
        .bind(run_id)
        .bind(pdo_kernel::clock::now_iso())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM events WHERE run_id = ?")
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    summaries::drop_in(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(())
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

/// The Runs started from the session of `node_id` in `run_id`: one indexed
/// read of the `run_started` events that carry this provenance (ADR-0064).
pub async fn child_run_ids(
    db: &sqlx::SqlitePool,
    run_id: &str,
    node_id: &str,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT run_id FROM events WHERE kind = 'run_started' \
         AND json_extract(payload, '$.parent_run_id') = ? \
         AND json_extract(payload, '$.parent_node_id') = ?",
    )
    .bind(run_id)
    .bind(node_id)
    .fetch_all(db)
    .await?)
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

    pub(crate) fn pool(&self) -> &sqlx::SqlitePool {
        &self.db
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

    fn child_run_ids<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(child_run_ids(&self.db, run_id, node_id))
    }

    fn forget<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(forget(&self.db, run_id))
    }

    fn summaries(&self) -> BoxFuture<'_, Result<Vec<RunSummary>>> {
        Box::pin(summaries::list(&self.db))
    }

    fn summary<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<Option<RunSummary>>> {
        Box::pin(summaries::get(&self.db, run_id))
    }

    fn summaries_current(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(summaries::current(&self.db))
    }

    fn replace_summaries(&self, all: Vec<RunSummary>) -> BoxFuture<'_, Result<()>> {
        Box::pin(summaries::replace(&self.db, all))
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
    async fn the_children_of_a_node_are_the_runs_started_from_its_session() {
        let store = store().await;
        let started = |run_id: &str, node: &str| {
            event(
                run_id,
                EventKind::RunStarted,
                Some(serde_json::json!({ "parent_run_id": "parent", "parent_node_id": node })),
            )
        };
        for e in [
            started("child-a", "worker"),
            started("child-b", "worker"),
            started("other", "reviewer"),
        ] {
            store.append(&e).await.unwrap();
        }
        let port: &dyn EventStore = &store;
        let mut ids = port.child_run_ids("parent", "worker").await.unwrap();
        ids.sort();
        assert_eq!(ids, vec!["child-a", "child-b"]);
        assert!(port
            .child_run_ids("parent", "nobody")
            .await
            .unwrap()
            .is_empty());
        // A forgotten child leaves the node: its log is gone.
        port.forget("child-b").await.unwrap();
        assert_eq!(
            port.child_run_ids("parent", "worker").await.unwrap(),
            vec!["child-a"]
        );
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
    async fn forgetting_a_run_purges_its_events_and_tombstones_it() {
        let store = store().await;
        store
            .append(&event("gone", EventKind::RunStarted, None))
            .await
            .unwrap();
        store
            .append(&event("kept", EventKind::RunStarted, None))
            .await
            .unwrap();
        store.forget("gone").await.unwrap();
        assert!(store.events("gone").await.unwrap().is_empty());
        assert!(store.is_forgotten("gone").await.unwrap());
        assert_eq!(store.events("kept").await.unwrap().len(), 1);
        assert!(store
            .append(&event("gone", EventKind::NodeStarted, None))
            .await
            .is_err());
        // Forgetting twice is harmless.
        store.forget("gone").await.unwrap();
    }

    #[tokio::test]
    async fn an_append_writes_its_run_s_summary_in_its_transaction() {
        let store = store().await;
        assert!(store.summaries().await.unwrap().is_empty());
        store
            .append(&event(
                "r1",
                EventKind::RunStarted,
                Some(serde_json::json!({ "pipeline_name": "p", "target_repo": "/repo" })),
            ))
            .await
            .unwrap();
        // A log without `RunStarted` projects no Run: no Résumé.
        store
            .append(&event("orphan", EventKind::NodeStarted, None))
            .await
            .unwrap();
        let summary = store.summary("r1").await.unwrap().unwrap();
        assert_eq!(summary.pipeline_name, "p");
        assert_eq!(summary.target_repo.as_deref(), Some("/repo"));
        assert_eq!(store.summary("orphan").await.unwrap(), None);
        assert_eq!(store.summaries().await.unwrap(), vec![summary]);

        // A refused append (forgotten Run) writes nothing; forgetting drops
        // the Résumé with the log.
        store.forget("r1").await.unwrap();
        assert_eq!(store.summary("r1").await.unwrap(), None);
        assert!(store
            .append(&event(
                "r1",
                EventKind::RunStarted,
                Some(serde_json::json!({}))
            ))
            .await
            .is_err());
        assert_eq!(store.summary("r1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_summaries_are_current_from_a_rebuild_on_until_a_write_they_missed() {
        let store = store().await;
        assert!(!store.summaries_current().await.unwrap(), "never built");
        store.replace_summaries(Vec::new()).await.unwrap();
        assert!(store.summaries_current().await.unwrap());

        // The appends keep them current.
        let started = event(
            "r1",
            EventKind::RunStarted,
            Some(serde_json::json!({ "pipeline_name": "p" })),
        );
        store.append(&started).await.unwrap();
        store
            .append(&event("r1", EventKind::NodeStarted, None))
            .await
            .unwrap();
        assert!(store.summaries_current().await.unwrap());

        // A daemon that does not keep them writes to the log: not current.
        sqlx::query("INSERT INTO events (run_id, ts, kind) VALUES ('r2', 't', 'run_started')")
            .execute(&store.db)
            .await
            .unwrap();
        assert!(!store.summaries_current().await.unwrap());
        // And later appends do not hide it.
        store
            .append(&event("r1", EventKind::NodeCompleted, None))
            .await
            .unwrap();
        assert!(!store.summaries_current().await.unwrap());

        // Rebuilt; then such a daemon purges a Run's log but not its Résumé.
        store.replace_summaries(Vec::new()).await.unwrap();
        assert!(store.summaries_current().await.unwrap());
        sqlx::query("INSERT INTO run_summaries (run_id, summary) VALUES ('ghost', '{}')")
            .execute(&store.db)
            .await
            .unwrap();
        assert!(!store.summaries_current().await.unwrap());
    }

    #[tokio::test]
    async fn a_read_catches_up_with_events_written_behind_the_daemon() {
        use crate::domain::run_summary::summarize;
        let store = store().await;
        store.replace_summaries(Vec::new()).await.unwrap();
        store
            .append(&event(
                "r1",
                EventKind::RunStarted,
                Some(serde_json::json!({ "pipeline_name": "p" })),
            ))
            .await
            .unwrap();

        // A tool seeds the database of the running instance: two Runs, one
        // event of r1.
        for (run, kind, payload) in [
            ("r2", "run_started", Some(r#"{"pipeline_name":"seeded"}"#)),
            ("r3", "run_started", Some(r#"{"pipeline_name":"seeded"}"#)),
            ("r1", "run_completed", None),
        ] {
            sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, 't', ?, ?)")
                .bind(run)
                .bind(kind)
                .bind(payload)
                .execute(&store.db)
                .await
                .unwrap();
        }
        assert!(!store.summaries_current().await.unwrap());

        // The daemon appends behind them: its Résumé is written, the mark
        // stays behind the events it did not write.
        store
            .append(&event("r3", EventKind::NodeStarted, None))
            .await
            .unwrap();
        assert!(!store.summaries_current().await.unwrap());

        // The next read sees them all, and the mark is the log's again.
        let r2 = store.summary("r2").await.unwrap().unwrap();
        assert_eq!(r2.pipeline_name, "seeded");
        let mut listed: Vec<String> = store
            .summaries()
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.run_id)
            .collect();
        listed.sort();
        assert_eq!(listed, ["r1", "r2", "r3"]);
        for run in ["r1", "r3"] {
            assert_eq!(
                store.summary(run).await.unwrap().unwrap(),
                summarize(&store.events(run).await.unwrap()).unwrap()
            );
        }
        assert!(store.summaries_current().await.unwrap());

        // The appends keep them current from there.
        store
            .append(&event("r2", EventKind::NodeStarted, None))
            .await
            .unwrap();
        assert!(store.summaries_current().await.unwrap());
    }

    #[tokio::test]
    async fn init_is_idempotent() {
        let store = store().await;
        init(&store.db).await.unwrap();
    }
}
