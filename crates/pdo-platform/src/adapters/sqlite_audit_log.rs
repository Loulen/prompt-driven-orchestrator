//! The audit journal in SQLite: the append-only `audit_log` table of `pdo.db`,
//! without `run_id`.

use sqlx::{Row, SqlitePool};

use crate::application::ports::{AuditLog, BoxFuture, StorageError};
use crate::domain::audit::{AuditEntry, AuditFilter, NewAuditEntry};

pub struct SqliteAuditLog {
    db: SqlitePool,
}

impl SqliteAuditLog {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn storage(error: sqlx::Error) -> StorageError {
    StorageError(error.to_string())
}

/// A malformed `before`/`after` TEXT (never written by `record`, but not worth a
/// panic) degrades to `None`.
fn row_to_entry(row: &sqlx::sqlite::SqliteRow) -> Result<AuditEntry, sqlx::Error> {
    let before: Option<String> = row.try_get("before")?;
    let after: Option<String> = row.try_get("after")?;
    Ok(AuditEntry {
        id: row.get("id"),
        ts: row.get("ts"),
        actor_hint: row.get("actor_hint"),
        action: row.get("action"),
        target_kind: row.try_get("target_kind")?,
        target_id: row.try_get("target_id")?,
        before: before.and_then(|s| serde_json::from_str(&s).ok()),
        after: after.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

impl AuditLog for SqliteAuditLog {
    /// `CREATE TABLE IF NOT EXISTS`, no `ALTER`, no backfill: no risk on a
    /// production `pdo.db`.
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async move {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS audit_log (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    ts          TEXT NOT NULL,
                    actor_hint  TEXT NOT NULL,
                    action      TEXT NOT NULL,
                    target_kind TEXT,
                    target_id   TEXT,
                    before      TEXT,
                    after       TEXT
                )",
            )
            .execute(&self.db)
            .await
            .map_err(storage)?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(ts)")
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(())
        })
    }

    /// `before`/`after` are stored as JSON TEXT (SQLite has no real JSON type).
    fn record(&self, e: NewAuditEntry) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async move {
            let before = e.before.as_ref().map(|v| v.to_string());
            let after = e.after.as_ref().map(|v| v.to_string());
            sqlx::query(
                "INSERT INTO audit_log (ts, actor_hint, action, target_kind, target_id, before, after)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(pdo_kernel::clock::now_iso())
            .bind(&e.actor_hint)
            .bind(&e.action)
            .bind(&e.target_kind)
            .bind(&e.target_id)
            .bind(before)
            .bind(after)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    /// `id DESC` alone is a total order (`INTEGER PRIMARY KEY AUTOINCREMENT`):
    /// no tiebreak on `ts`.
    fn list<'a>(
        &'a self,
        filter: &'a AuditFilter,
    ) -> BoxFuture<'a, Result<Vec<AuditEntry>, StorageError>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id, ts, actor_hint, action, target_kind, target_id, before, after
                 FROM audit_log
                 WHERE (?1 IS NULL OR ts >= ?1)
                   AND (?2 IS NULL OR ts < ?2)
                   AND (?3 IS NULL OR target_kind = ?3)
                   AND (?4 IS NULL OR target_id = ?4)
                 ORDER BY id DESC
                 LIMIT ?5",
            )
            .bind(&filter.from)
            .bind(&filter.to)
            .bind(&filter.target_kind)
            .bind(&filter.target_id)
            .bind(filter.limit)
            .fetch_all(&self.db)
            .await
            .map_err(storage)?;
            rows.iter()
                .map(row_to_entry)
                .collect::<Result<_, _>>()
                .map_err(storage)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn journal() -> SqliteAuditLog {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let log = SqliteAuditLog::new(db);
        log.init().await.unwrap();
        log
    }

    fn trigger_entry(action: &str, id: &str) -> NewAuditEntry {
        NewAuditEntry {
            actor_hint: "ui".to_string(),
            action: action.to_string(),
            target_kind: Some("trigger".to_string()),
            target_id: Some(id.to_string()),
            before: None,
            after: None,
        }
    }

    fn all() -> AuditFilter {
        AuditFilter::default()
    }

    #[tokio::test]
    async fn record_then_list_roundtrips_before_and_after_json() {
        // A patch-shaped entry carries both snapshots; `list` must give them back
        // byte-for-byte as JSON, not as re-escaped strings.
        let log = journal().await;
        log.record(NewAuditEntry {
            before: Some(serde_json::json!({"enabled": true, "name": "nightly"})),
            after: Some(serde_json::json!({"enabled": false, "name": "nightly"})),
            ..trigger_entry("trigger.updated", "t-1")
        })
        .await
        .unwrap();

        let rows = log.list(&all()).await.unwrap();
        assert_eq!(rows.len(), 1);
        let e = &rows[0];
        assert_eq!(e.actor_hint, "ui");
        assert_eq!(e.action, "trigger.updated");
        assert_eq!(e.target_kind.as_deref(), Some("trigger"));
        assert_eq!(e.target_id.as_deref(), Some("t-1"));
        assert_eq!(
            e.before.as_ref().unwrap()["enabled"],
            serde_json::json!(true)
        );
        assert_eq!(
            e.after.as_ref().unwrap()["enabled"],
            serde_json::json!(false)
        );
        assert!(!e.ts.is_empty(), "ts is stamped by now_iso()");
    }

    #[tokio::test]
    async fn create_and_delete_null_snapshots_survive() {
        // create → before NULL, after set; delete → before set, after NULL. Both
        // NULLs must round-trip as `None`, not as a JSON `null` Value.
        let log = journal().await;
        log.record(NewAuditEntry {
            after: Some(serde_json::json!({"id": "t-1"})),
            ..trigger_entry("trigger.created", "t-1")
        })
        .await
        .unwrap();
        log.record(NewAuditEntry {
            before: Some(serde_json::json!({"id": "t-1"})),
            ..trigger_entry("trigger.deleted", "t-1")
        })
        .await
        .unwrap();

        let rows = log.list(&all()).await.unwrap();
        // newest-first: delete then create.
        assert_eq!(rows[0].action, "trigger.deleted");
        assert!(rows[0].before.is_some());
        assert!(rows[0].after.is_none());
        assert_eq!(rows[1].action, "trigger.created");
        assert!(rows[1].before.is_none());
        assert!(rows[1].after.is_some());
    }

    #[tokio::test]
    async fn list_is_newest_first_by_id_and_limited() {
        let log = journal().await;
        for i in 0..10 {
            log.record(trigger_entry("trigger.updated", &format!("t-{i}")))
                .await
                .unwrap();
        }
        let rows = log.list(&all()).await.unwrap();
        let ids: Vec<i64> = rows.iter().map(|e| e.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.reverse();
        assert_eq!(ids, sorted, "id DESC is a total order, newest first");
        let three = AuditFilter { limit: 3, ..all() };
        assert_eq!(
            log.list(&three).await.unwrap().len(),
            3,
            "LIMIT bounds the feed"
        );
    }

    #[tokio::test]
    async fn target_filters_narrow_to_one_trigger() {
        let log = journal().await;
        log.record(trigger_entry("trigger.updated", "t-1"))
            .await
            .unwrap();
        log.record(trigger_entry("trigger.updated", "t-2"))
            .await
            .unwrap();
        log.record(NewAuditEntry {
            target_kind: Some("instance".to_string()),
            target_id: None,
            ..trigger_entry("triggers.pause_changed", "ignored")
        })
        .await
        .unwrap();

        let only_t1 = AuditFilter {
            target_kind: Some("trigger".into()),
            target_id: Some("t-1".into()),
            ..all()
        };
        let rows = log.list(&only_t1).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target_id.as_deref(), Some("t-1"));

        let only_instance = AuditFilter {
            target_kind: Some("instance".into()),
            ..all()
        };
        let rows = log.list(&only_instance).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, "triggers.pause_changed");
    }

    #[tokio::test]
    async fn from_to_window_is_half_open() {
        // `[from, to)`: `from` inclusive, `to` exclusive. Bind the rows' own
        // timestamps as bounds to assert the boundary semantics precisely.
        let log = journal().await;
        for i in 0..3 {
            log.record(trigger_entry("trigger.updated", &format!("t-{i}")))
                .await
                .unwrap();
        }
        let every = log.list(&all()).await.unwrap();
        assert_eq!(every.len(), 3);
        let mut ts: Vec<String> = every.iter().map(|e| e.ts.clone()).collect();
        ts.sort();

        let from_mid = AuditFilter {
            from: Some(ts[1].clone()),
            ..all()
        };
        let rows = log.list(&from_mid).await.unwrap();
        assert!(rows.iter().all(|e| e.ts >= ts[1]), "from is inclusive");

        let to_max = AuditFilter {
            to: Some(ts[2].clone()),
            ..all()
        };
        let rows = log.list(&to_max).await.unwrap();
        assert!(rows.iter().all(|e| e.ts < ts[2]), "to is exclusive");
    }

    #[tokio::test]
    async fn a_closed_base_is_an_error_not_a_panic() {
        let log = journal().await;
        log.db.close().await;
        assert!(log
            .record(trigger_entry("trigger.updated", "t-1"))
            .await
            .is_err());
    }
}
