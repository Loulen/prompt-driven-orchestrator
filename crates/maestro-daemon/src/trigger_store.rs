//! Persistence for Triggers and their fire history.
//!
//! A Trigger is config + mutable scheduling state (CONTEXT.md → *Trigger*), so
//! it lives in SQLite (`triggers` + `trigger_fires`) alongside the event log
//! rather than as a YAML artifact. This does not violate event-sourcing: the
//! event log remains the source of truth for *Run* state; a Trigger merely
//! *produces* Runs.
//!
//! The public API is intentionally small: table creation, CRUD, the scheduler's
//! `due_triggers(now)` query, and the fire-audit helpers (`record_fire`,
//! `set_next_fire`, `set_enabled`).

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// A persisted Trigger row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub name: String,
    /// Library pipeline id the Trigger fires.
    pub pipeline_id: String,
    /// Pipeline display name (denormalised for list rendering).
    #[serde(default)]
    pub pipeline_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub input_template: String,
    /// Variable overrides as a JSON object (stored as a JSON string).
    #[serde(default)]
    pub variables: serde_json::Value,
    /// 5-field cron expression.
    pub cron: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_command: Option<String>,
    /// `"skip"` (default) or `"allow"`.
    pub overlap_policy: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_outcome: Option<String>,
}

/// The fields supplied at creation time; scheduling state is derived/initial.
#[derive(Debug, Clone)]
pub struct NewTrigger {
    pub name: String,
    pub pipeline_id: String,
    pub pipeline_name: String,
    pub target_repo: Option<String>,
    pub source_branch: Option<String>,
    pub input_template: String,
    pub variables: serde_json::Value,
    pub cron: String,
    pub guard_command: Option<String>,
    pub overlap_policy: String,
    /// First scheduled fire, computed by the caller from the cron expression.
    pub next_fire_at: Option<String>,
}

/// One audit row in `trigger_fires`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerFire {
    pub id: i64,
    pub trigger_id: String,
    pub ts: String,
    /// `fired` / `skipped-overlap` / `guard-exit-nonzero` / `guard-error` / `error`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// What happened on a tick, persisted to the audit table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRecord {
    pub outcome: String,
    pub reason: Option<String>,
    pub run_id: Option<String>,
}

/// Create the `triggers` and `trigger_fires` tables if they do not exist.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS triggers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            pipeline_id TEXT NOT NULL,
            pipeline_name TEXT NOT NULL DEFAULT '',
            target_repo TEXT,
            source_branch TEXT,
            input_template TEXT NOT NULL DEFAULT '',
            variables JSON NOT NULL DEFAULT '{}',
            cron TEXT NOT NULL,
            guard_command TEXT,
            overlap_policy TEXT NOT NULL DEFAULT 'skip',
            enabled INTEGER NOT NULL DEFAULT 1,
            next_fire_at TEXT,
            last_fired_at TEXT,
            last_outcome TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS trigger_fires (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trigger_id TEXT NOT NULL,
            ts TEXT NOT NULL,
            outcome TEXT NOT NULL,
            reason TEXT,
            run_id TEXT
        )",
    )
    .execute(db)
    .await?;

    Ok(())
}

/// Generate a Trigger id (`trg-<ts>-<short uuid>`).
pub fn generate_trigger_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("trg-{ts}-{short}")
}

/// Insert a new Trigger, returning the stored row.
pub async fn create(db: &SqlitePool, new: NewTrigger) -> Result<Trigger, sqlx::Error> {
    let id = generate_trigger_id();
    let now = crate::event_log::now_iso();
    let variables_str = serde_json::to_string(&new.variables).unwrap_or_else(|_| "{}".to_string());

    sqlx::query(
        "INSERT INTO triggers
            (id, name, pipeline_id, pipeline_name, target_repo, source_branch,
             input_template, variables, cron, guard_command, overlap_policy,
             enabled, next_fire_at, last_fired_at, last_outcome, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, NULL, NULL, ?)",
    )
    .bind(&id)
    .bind(&new.name)
    .bind(&new.pipeline_id)
    .bind(&new.pipeline_name)
    .bind(&new.target_repo)
    .bind(&new.source_branch)
    .bind(&new.input_template)
    .bind(&variables_str)
    .bind(&new.cron)
    .bind(&new.guard_command)
    .bind(&new.overlap_policy)
    .bind(&new.next_fire_at)
    .bind(&now)
    .execute(db)
    .await?;

    get(db, &id)
        .await
        .map(|t| t.expect("just-inserted trigger"))
}

fn row_to_trigger(row: &sqlx::sqlite::SqliteRow) -> Trigger {
    let variables_str: String = row.get("variables");
    let variables = serde_json::from_str(&variables_str).unwrap_or(serde_json::json!({}));
    Trigger {
        id: row.get("id"),
        name: row.get("name"),
        pipeline_id: row.get("pipeline_id"),
        pipeline_name: row.get("pipeline_name"),
        target_repo: row.get("target_repo"),
        source_branch: row.get("source_branch"),
        input_template: row.get("input_template"),
        variables,
        cron: row.get("cron"),
        guard_command: row.get("guard_command"),
        overlap_policy: row.get("overlap_policy"),
        enabled: row.get::<i64, _>("enabled") != 0,
        next_fire_at: row.get("next_fire_at"),
        last_fired_at: row.get("last_fired_at"),
        last_outcome: row.get("last_outcome"),
    }
}

/// Fetch one Trigger by id.
pub async fn get(db: &SqlitePool, id: &str) -> Result<Option<Trigger>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM triggers WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_trigger))
}

/// List all Triggers, newest first.
pub async fn list(db: &SqlitePool) -> Result<Vec<Trigger>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM triggers ORDER BY created_at DESC")
        .fetch_all(db)
        .await?;
    Ok(rows.iter().map(row_to_trigger).collect())
}

/// Delete a Trigger by id; returns whether a row was removed.
pub async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM triggers WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Enabled Triggers whose `next_fire_at` is at or before `now`. The scheduler's
/// central query.
pub async fn due_triggers(db: &SqlitePool, now: &str) -> Result<Vec<Trigger>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM triggers
         WHERE enabled = 1 AND next_fire_at IS NOT NULL AND next_fire_at <= ?
         ORDER BY next_fire_at ASC",
    )
    .bind(now)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_trigger).collect())
}

/// Record a fire-audit row and roll up `last_fired_at`/`last_outcome` onto the
/// Trigger. `last_fired_at` is only advanced for an actual fire.
pub async fn record_fire(
    db: &SqlitePool,
    trigger_id: &str,
    record: &FireRecord,
) -> Result<(), sqlx::Error> {
    let ts = crate::event_log::now_iso();
    sqlx::query(
        "INSERT INTO trigger_fires (trigger_id, ts, outcome, reason, run_id)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(trigger_id)
    .bind(&ts)
    .bind(&record.outcome)
    .bind(&record.reason)
    .bind(&record.run_id)
    .execute(db)
    .await?;

    if record.outcome == "fired" {
        sqlx::query("UPDATE triggers SET last_fired_at = ?, last_outcome = ? WHERE id = ?")
            .bind(&ts)
            .bind(&record.outcome)
            .bind(trigger_id)
            .execute(db)
            .await?;
    } else {
        sqlx::query("UPDATE triggers SET last_outcome = ? WHERE id = ?")
            .bind(&record.outcome)
            .bind(trigger_id)
            .execute(db)
            .await?;
    }
    Ok(())
}

/// Fire history for a Trigger, newest first.
pub async fn fire_history(
    db: &SqlitePool,
    trigger_id: &str,
) -> Result<Vec<TriggerFire>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, trigger_id, ts, outcome, reason, run_id
         FROM trigger_fires WHERE trigger_id = ? ORDER BY id DESC",
    )
    .bind(trigger_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .iter()
        .map(|row| TriggerFire {
            id: row.get("id"),
            trigger_id: row.get("trigger_id"),
            ts: row.get("ts"),
            outcome: row.get("outcome"),
            reason: row.get("reason"),
            run_id: row.get("run_id"),
        })
        .collect())
}

/// Update the next scheduled fire.
pub async fn set_next_fire(
    db: &SqlitePool,
    trigger_id: &str,
    next_fire_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE triggers SET next_fire_at = ? WHERE id = ?")
        .bind(next_fire_at)
        .bind(trigger_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Enable or disable a Trigger.
pub async fn set_enabled(
    db: &SqlitePool,
    trigger_id: &str,
    enabled: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE triggers SET enabled = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(trigger_id)
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init(&db).await.unwrap();
        db
    }

    fn sample(name: &str, cron: &str) -> NewTrigger {
        NewTrigger {
            name: name.to_string(),
            pipeline_id: "lib-pipe-1".to_string(),
            pipeline_name: "Auditor".to_string(),
            target_repo: Some("/repos/foo".to_string()),
            source_branch: Some("main".to_string()),
            input_template: "audit the codebase".to_string(),
            variables: serde_json::json!({"depth": "full"}),
            cron: cron.to_string(),
            guard_command: None,
            overlap_policy: "skip".to_string(),
            next_fire_at: Some("2026-06-06T10:00:00.000Z".to_string()),
        }
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let created = create(&db, sample("nightly audit", "0 9 * * *"))
            .await
            .unwrap();
        let fetched = get(&db, &created.id).await.unwrap().unwrap();
        assert_eq!(fetched, created);
        assert_eq!(fetched.name, "nightly audit");
        assert_eq!(fetched.cron, "0 9 * * *");
        assert!(fetched.enabled);
        assert_eq!(fetched.variables, serde_json::json!({"depth": "full"}));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let db = test_db().await;
        assert!(get(&db, "trg-nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_returns_all_triggers() {
        let db = test_db().await;
        create(&db, sample("a", "0 9 * * *")).await.unwrap();
        create(&db, sample("b", "*/15 * * * *")).await.unwrap();
        let all = list(&db).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn delete_removes_a_trigger() {
        let db = test_db().await;
        let t = create(&db, sample("doomed", "0 9 * * *")).await.unwrap();
        assert!(delete(&db, &t.id).await.unwrap());
        assert!(get(&db, &t.id).await.unwrap().is_none());
        // Deleting again is a no-op.
        assert!(!delete(&db, &t.id).await.unwrap());
    }

    #[tokio::test]
    async fn due_triggers_selects_enabled_and_past_due() {
        let db = test_db().await;
        // Due (next_fire in the past) and enabled.
        let mut due = sample("due", "* * * * *");
        due.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
        let due = create(&db, due).await.unwrap();
        // Not yet due (next_fire in the future).
        let mut future = sample("future", "* * * * *");
        future.next_fire_at = Some("2999-01-01T00:00:00.000Z".to_string());
        create(&db, future).await.unwrap();
        // Due but disabled.
        let mut disabled = sample("disabled", "* * * * *");
        disabled.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
        let disabled = create(&db, disabled).await.unwrap();
        set_enabled(&db, &disabled.id, false).await.unwrap();

        let selected = due_triggers(&db, "2026-06-06T10:00:00.000Z").await.unwrap();
        let ids: Vec<&str> = selected.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec![due.id.as_str()]);
    }

    #[tokio::test]
    async fn record_fire_writes_audit_and_rolls_up_outcome() {
        let db = test_db().await;
        let t = create(&db, sample("audited", "* * * * *")).await.unwrap();

        record_fire(
            &db,
            &t.id,
            &FireRecord {
                outcome: "skipped-overlap".to_string(),
                reason: Some("previous run still active".to_string()),
                run_id: None,
            },
        )
        .await
        .unwrap();
        record_fire(
            &db,
            &t.id,
            &FireRecord {
                outcome: "fired".to_string(),
                reason: None,
                run_id: Some("20260606-100000-abc1234".to_string()),
            },
        )
        .await
        .unwrap();

        let history = fire_history(&db, &t.id).await.unwrap();
        assert_eq!(history.len(), 2);
        // Newest first.
        assert_eq!(history[0].outcome, "fired");
        assert_eq!(
            history[0].run_id.as_deref(),
            Some("20260606-100000-abc1234")
        );
        assert_eq!(history[1].outcome, "skipped-overlap");

        let after = get(&db, &t.id).await.unwrap().unwrap();
        assert_eq!(after.last_outcome.as_deref(), Some("fired"));
        assert!(after.last_fired_at.is_some());
    }

    #[tokio::test]
    async fn non_fire_outcome_updates_last_outcome_but_not_last_fired_at() {
        let db = test_db().await;
        let t = create(&db, sample("skippy", "* * * * *")).await.unwrap();
        record_fire(
            &db,
            &t.id,
            &FireRecord {
                outcome: "guard-exit-nonzero".to_string(),
                reason: Some("no work".to_string()),
                run_id: None,
            },
        )
        .await
        .unwrap();
        let after = get(&db, &t.id).await.unwrap().unwrap();
        assert_eq!(after.last_outcome.as_deref(), Some("guard-exit-nonzero"));
        assert!(after.last_fired_at.is_none());
    }

    #[tokio::test]
    async fn set_next_fire_updates_schedule() {
        let db = test_db().await;
        let t = create(&db, sample("rescheduled", "* * * * *"))
            .await
            .unwrap();
        set_next_fire(&db, &t.id, Some("2027-01-01T00:00:00.000Z"))
            .await
            .unwrap();
        let after = get(&db, &t.id).await.unwrap().unwrap();
        assert_eq!(
            after.next_fire_at.as_deref(),
            Some("2027-01-01T00:00:00.000Z")
        );
        // A broken reference can clear next_fire so the trigger stops firing.
        set_next_fire(&db, &t.id, None).await.unwrap();
        assert!(get(&db, &t.id)
            .await
            .unwrap()
            .unwrap()
            .next_fire_at
            .is_none());
    }

    #[tokio::test]
    async fn set_enabled_toggles_and_pauses_firing() {
        let db = test_db().await;
        let mut due = sample("toggle", "* * * * *");
        due.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
        let t = create(&db, due).await.unwrap();
        set_enabled(&db, &t.id, false).await.unwrap();
        assert!(!get(&db, &t.id).await.unwrap().unwrap().enabled);
        // A disabled-but-due trigger is excluded from due_triggers.
        assert!(due_triggers(&db, "2026-06-06T10:00:00.000Z")
            .await
            .unwrap()
            .is_empty());
        set_enabled(&db, &t.id, true).await.unwrap();
        assert!(get(&db, &t.id).await.unwrap().unwrap().enabled);
    }
}
