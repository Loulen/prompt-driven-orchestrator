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
    /// Bounded-`allow` ceiling: max simultaneous live Runs of this Trigger (#239).
    /// `None` = unbounded (also the effective value under the `skip` policy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<i64>,
    pub enabled: bool,
    /// The next scheduled fire, as **canonical UTC RFC3339-millis** (`…Z`).
    /// Every writer (create/edit in `lib.rs`, the scheduler's `set_next_fire`)
    /// stores UTC, so a lexicographic string compare equals a chronological one.
    /// The due query ([`due_triggers`]) is nonetheless tz-normalised so a legacy
    /// or stray-offset row can never silently go dormant (#222).
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
    /// Bounded-`allow` ceiling (#239); `None` = unbounded.
    pub max_concurrent: Option<i64>,
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
    /// Guard diagnostics on a `guard-exit-nonzero` row (#244): what the guard
    /// printed and the exit status. NULL on every other outcome and on legacy
    /// rows; tail-capped to 16 KB each (see `guard_runner`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_stderr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_exit_code: Option<i32>,
}

/// What happened on a tick, persisted to the audit table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRecord {
    pub outcome: String,
    pub reason: Option<String>,
    pub run_id: Option<String>,
    /// Guard diagnostics, set only on a `guard-exit-nonzero` record (#244).
    pub guard_stdout: Option<String>,
    pub guard_stderr: Option<String>,
    pub guard_exit_code: Option<i32>,
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
            max_concurrent INTEGER,
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
            run_id TEXT,
            guard_stdout TEXT,
            guard_stderr TEXT,
            guard_exit_code INTEGER
        )",
    )
    .execute(db)
    .await?;

    // Additive migration (#239): a `~/.pdo/pdo.db` created before this column
    // existed got the table via `CREATE TABLE IF NOT EXISTS` above, which is a
    // no-op there — so the column must be added out-of-band. There is no
    // migration runner; this PRAGMA-guarded ALTER is the only durable path. The
    // guard keeps it idempotent (a bare `ALTER … ADD COLUMN` errors "duplicate
    // column name" on an already-migrated DB), and is preferred over swallowing
    // the ALTER error blindly — a swallowed error would hide genuine failures.
    let has_col = sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'")
        .fetch_optional(db)
        .await?
        .is_some();
    if !has_col {
        sqlx::query("ALTER TABLE triggers ADD COLUMN max_concurrent INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration (#244): the guard-output columns on `trigger_fires`.
    // Same PRAGMA-guarded `ALTER` precedent as `max_concurrent` above — a
    // pre-#244 `~/.pdo/pdo.db` got the table via `CREATE TABLE IF NOT EXISTS`,
    // a no-op there, so the columns must be added out-of-band or runtime
    // INSERT/SELECT would fail. Each guard keeps the ALTER idempotent.
    for (col, ddl) in [
        ("guard_stdout", "ALTER TABLE trigger_fires ADD COLUMN guard_stdout TEXT"),
        ("guard_stderr", "ALTER TABLE trigger_fires ADD COLUMN guard_stderr TEXT"),
        (
            "guard_exit_code",
            "ALTER TABLE trigger_fires ADD COLUMN guard_exit_code INTEGER",
        ),
    ] {
        let exists = sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = ?")
            .bind(col)
            .fetch_optional(db)
            .await?
            .is_some();
        if !exists {
            sqlx::query(ddl).execute(db).await?;
        }
    }

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
             max_concurrent, enabled, next_fire_at, last_fired_at, last_outcome, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, NULL, NULL, ?)",
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
    .bind(new.max_concurrent)
    .bind(&new.next_fire_at)
    .bind(&now)
    .execute(db)
    .await?;

    get(db, &id).await?.ok_or(sqlx::Error::RowNotFound)
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
        max_concurrent: row.get("max_concurrent"),
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
///
/// Comparison and ordering are **timezone-normalised** via `julianday()` rather
/// than a raw string compare (#222): `next_fire_at` is invariably canonical UTC
/// (`…Z`) — see [`Trigger::next_fire_at`] — so a string compare *would* be
/// correct, but a stray local-offset row (legacy data, or `chrono::Local::now()`
/// slipping back into a writer) sorts lexicographically *after* a `…Z` now-string
/// and would silently go dormant for hours. `julianday()` parses `Z`/`±HH:MM`/
/// fractional-second RFC3339 to a UTC instant, so any offset compares correctly.
/// `now` is a canonical-UTC RFC3339-millis now-string (`…Z`).
pub async fn due_triggers(db: &SqlitePool, now: &str) -> Result<Vec<Trigger>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM triggers
         WHERE enabled = 1 AND next_fire_at IS NOT NULL
               AND julianday(next_fire_at) <= julianday(?)
         ORDER BY julianday(next_fire_at) ASC",
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
        "INSERT INTO trigger_fires
            (trigger_id, ts, outcome, reason, run_id,
             guard_stdout, guard_stderr, guard_exit_code)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(trigger_id)
    .bind(&ts)
    .bind(&record.outcome)
    .bind(&record.reason)
    .bind(&record.run_id)
    .bind(&record.guard_stdout)
    .bind(&record.guard_stderr)
    .bind(record.guard_exit_code)
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
    // Cap the read (#244/D5): the new guard-output blobs make each row heavier
    // and a minute-cron trigger accrues ~1440 rows/day; the panel only ever
    // shows the recent tail. Newest-first, bounded to the latest 200.
    let rows = sqlx::query(
        "SELECT id, trigger_id, ts, outcome, reason, run_id,
                guard_stdout, guard_stderr, guard_exit_code
         FROM trigger_fires WHERE trigger_id = ? ORDER BY id DESC LIMIT 200",
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
            guard_stdout: row.get("guard_stdout"),
            guard_stderr: row.get("guard_stderr"),
            guard_exit_code: row.get("guard_exit_code"),
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

/// A partial config edit (#162). Every field is optional: `None` leaves the
/// stored value untouched. `next_fire_at` is double-wrapped so the caller can
/// distinguish "leave alone" (`None`) from "set to NULL" (`Some(None)`); the
/// route recomputes it whenever the schedule changes.
#[derive(Debug, Clone, Default)]
pub struct UpdateTrigger {
    pub name: Option<String>,
    /// Repoint the Trigger to a different library pipeline (#230). The route is
    /// responsible for validating the target exists; both `pipeline_id` and the
    /// denormalised `pipeline_name` are updated together so list rendering can't
    /// show a stale name.
    pub pipeline_id: Option<String>,
    pub pipeline_name: Option<String>,
    pub target_repo: Option<Option<String>>,
    pub source_branch: Option<Option<String>>,
    pub input_template: Option<String>,
    pub variables: Option<serde_json::Value>,
    pub cron: Option<String>,
    pub guard_command: Option<Option<String>>,
    pub overlap_policy: Option<String>,
    /// Bounded-`allow` ceiling (#239), double-wrapped like the other nullable
    /// fields: `None` leaves it, `Some(None)` clears to NULL, `Some(Some(n))` sets.
    pub max_concurrent: Option<Option<i64>>,
    pub next_fire_at: Option<Option<String>>,
}

impl UpdateTrigger {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.pipeline_id.is_none()
            && self.pipeline_name.is_none()
            && self.target_repo.is_none()
            && self.source_branch.is_none()
            && self.input_template.is_none()
            && self.variables.is_none()
            && self.cron.is_none()
            && self.guard_command.is_none()
            && self.overlap_policy.is_none()
            && self.max_concurrent.is_none()
            && self.next_fire_at.is_none()
    }
}

/// Apply a partial config edit to a Trigger. A no-op when no field is set.
pub async fn update(
    db: &SqlitePool,
    trigger_id: &str,
    edit: UpdateTrigger,
) -> Result<(), sqlx::Error> {
    if edit.is_empty() {
        return Ok(());
    }

    // Build the SET clause field-by-field, then bind in the same order so the
    // positional placeholders line up.
    let mut sets: Vec<&str> = Vec::new();
    if edit.name.is_some() {
        sets.push("name = ?");
    }
    if edit.pipeline_id.is_some() {
        sets.push("pipeline_id = ?");
    }
    if edit.pipeline_name.is_some() {
        sets.push("pipeline_name = ?");
    }
    if edit.target_repo.is_some() {
        sets.push("target_repo = ?");
    }
    if edit.source_branch.is_some() {
        sets.push("source_branch = ?");
    }
    if edit.input_template.is_some() {
        sets.push("input_template = ?");
    }
    if edit.variables.is_some() {
        sets.push("variables = ?");
    }
    if edit.cron.is_some() {
        sets.push("cron = ?");
    }
    if edit.guard_command.is_some() {
        sets.push("guard_command = ?");
    }
    if edit.overlap_policy.is_some() {
        sets.push("overlap_policy = ?");
    }
    if edit.max_concurrent.is_some() {
        sets.push("max_concurrent = ?");
    }
    if edit.next_fire_at.is_some() {
        sets.push("next_fire_at = ?");
    }

    let sql = format!("UPDATE triggers SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    if let Some(v) = &edit.name {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.pipeline_id {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.pipeline_name {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.target_repo {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.source_branch {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.input_template {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.variables {
        query = query.bind(serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()));
    }
    if let Some(v) = &edit.cron {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.guard_command {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.overlap_policy {
        query = query.bind(v.clone());
    }
    if let Some(v) = &edit.max_concurrent {
        query = query.bind(*v);
    }
    if let Some(v) = &edit.next_fire_at {
        query = query.bind(v.clone());
    }
    query = query.bind(trigger_id);
    query.execute(db).await?;
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
            max_concurrent: None,
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
    async fn due_triggers_includes_a_past_due_local_offset_row() {
        // Regression for #222. A row stored with a local offset (a CEST box's
        // `chrono::Local::now()` before the UTC write-side fix) represents an
        // instant already in the past, yet sorts lexicographically *after* the
        // UTC now-string. A raw string compare drops it → silent dormancy; the
        // tz-normalised `julianday` compare keeps it. Fails pre-fix on any host
        // (it does not rely on the test machine being non-UTC).
        let db = test_db().await;
        let mut t = sample("legacy-offset", "* * * * *");
        // 19:15 +02:00 == 17:15Z, i.e. before the 17:30Z "now" → genuinely due.
        t.next_fire_at = Some("2026-06-18T19:15:00.000+02:00".to_string());
        let t = create(&db, t).await.unwrap();

        let now = "2026-06-18T17:30:00.000Z";
        // The bug's precondition: the stored string sorts *after* `now`.
        assert!(
            t.next_fire_at.as_deref().unwrap() > now,
            "precondition: the local-offset string must sort after the UTC now-string"
        );

        let due = due_triggers(&db, now).await.unwrap();
        assert_eq!(
            due.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec![t.id.as_str()],
            "a past-due local-offset row must still be selected (tz-normalised compare)"
        );
    }

    #[tokio::test]
    async fn due_triggers_orders_by_instant_not_string() {
        // Ordering must also be tz-normalised: an earlier instant carried on a
        // `+02:00` offset must sort before a later `…Z` instant even though it
        // sorts *after* it as a string.
        let db = test_db().await;
        let mut early = sample("early", "* * * * *");
        early.next_fire_at = Some("2026-06-18T19:00:00.000+02:00".to_string()); // 17:00Z
        let early = create(&db, early).await.unwrap();
        let mut late = sample("late", "* * * * *");
        late.next_fire_at = Some("2026-06-18T17:30:00.000Z".to_string()); // 17:30Z
        let late = create(&db, late).await.unwrap();

        let due = due_triggers(&db, "2026-06-18T18:00:00.000Z").await.unwrap();
        assert_eq!(
            due.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec![early.id.as_str(), late.id.as_str()],
            "due triggers must be ordered by instant (17:00Z before 17:30Z), not by string"
        );
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
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
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
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
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
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
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
    async fn update_edits_config_fields_and_leaves_others_untouched() {
        let db = test_db().await;
        let t = create(&db, sample("editable", "0 9 * * *")).await.unwrap();

        // Edit the schedule, input template and overlap policy; recompute the
        // next fire. Fields left `None` keep their prior value.
        update(
            &db,
            &t.id,
            UpdateTrigger {
                cron: Some("*/15 * * * *".to_string()),
                input_template: Some("new instruction".to_string()),
                overlap_policy: Some("allow".to_string()),
                next_fire_at: Some(Some("2027-03-01T00:00:00.000Z".to_string())),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let after = get(&db, &t.id).await.unwrap().unwrap();
        assert_eq!(after.cron, "*/15 * * * *");
        assert_eq!(after.input_template, "new instruction");
        assert_eq!(after.overlap_policy, "allow");
        assert_eq!(
            after.next_fire_at.as_deref(),
            Some("2027-03-01T00:00:00.000Z")
        );
        // Untouched fields survive.
        assert_eq!(after.name, "editable");
        assert_eq!(after.pipeline_id, "lib-pipe-1");
        assert_eq!(after.target_repo.as_deref(), Some("/repos/foo"));
        assert!(after.enabled);
    }

    #[tokio::test]
    async fn update_can_clear_a_nullable_field() {
        let db = test_db().await;
        let mut s = sample("guarded", "0 9 * * *");
        s.guard_command = Some("gh issue list".to_string());
        let t = create(&db, s).await.unwrap();
        assert_eq!(
            get(&db, &t.id)
                .await
                .unwrap()
                .unwrap()
                .guard_command
                .as_deref(),
            Some("gh issue list")
        );

        // Some(None) clears the guard to NULL; an unrelated edit leaves it alone.
        update(
            &db,
            &t.id,
            UpdateTrigger {
                guard_command: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(get(&db, &t.id)
            .await
            .unwrap()
            .unwrap()
            .guard_command
            .is_none());
    }

    #[tokio::test]
    async fn update_can_repoint_pipeline() {
        // #230: a Trigger must be movable to a different pipeline. Both the id and
        // the denormalised display name update together; unrelated fields survive.
        let db = test_db().await;
        let t = create(&db, sample("repointable", "0 9 * * *")).await.unwrap();
        assert_eq!(t.pipeline_id, "lib-pipe-1");
        assert_eq!(t.pipeline_name, "Auditor");

        update(
            &db,
            &t.id,
            UpdateTrigger {
                pipeline_id: Some("lib-pipe-2".to_string()),
                pipeline_name: Some("Bugfixer".to_string()),
                next_fire_at: Some(Some("2027-03-01T00:00:00.000Z".to_string())),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let after = get(&db, &t.id).await.unwrap().unwrap();
        assert_eq!(after.pipeline_id, "lib-pipe-2");
        assert_eq!(after.pipeline_name, "Bugfixer");
        // A revived next fire (the dormant-on-rename recovery path).
        assert_eq!(
            after.next_fire_at.as_deref(),
            Some("2027-03-01T00:00:00.000Z")
        );
        // Everything else is left untouched.
        assert_eq!(after.name, "repointable");
        assert_eq!(after.cron, "0 9 * * *");
        assert_eq!(after.input_template, "audit the codebase");
        assert_eq!(after.target_repo.as_deref(), Some("/repos/foo"));
        assert!(after.enabled);
    }

    #[tokio::test]
    async fn update_with_no_fields_is_a_noop() {
        let db = test_db().await;
        let t = create(&db, sample("stable", "0 9 * * *")).await.unwrap();
        update(&db, &t.id, UpdateTrigger::default()).await.unwrap();
        assert_eq!(get(&db, &t.id).await.unwrap().unwrap(), t);
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

    // --- #239: bounded-`allow` max_concurrent persistence ---

    #[tokio::test]
    async fn create_then_get_round_trips_max_concurrent() {
        let db = test_db().await;

        let mut bounded = sample("bounded", "0 9 * * *");
        bounded.overlap_policy = "allow".to_string();
        bounded.max_concurrent = Some(3);
        let created = create(&db, bounded).await.unwrap();
        assert_eq!(created.max_concurrent, Some(3));
        assert_eq!(
            get(&db, &created.id).await.unwrap().unwrap().max_concurrent,
            Some(3)
        );

        // The default (unbounded) round-trips as NULL.
        let unbounded = create(&db, sample("unbounded", "0 9 * * *"))
            .await
            .unwrap();
        assert_eq!(unbounded.max_concurrent, None);
        assert_eq!(
            get(&db, &unbounded.id).await.unwrap().unwrap().max_concurrent,
            None
        );
    }

    #[tokio::test]
    async fn update_sets_and_clears_max_concurrent() {
        let db = test_db().await;
        let t = create(&db, sample("capped", "0 9 * * *")).await.unwrap();
        assert_eq!(t.max_concurrent, None);

        // Some(Some(n)) sets it.
        update(
            &db,
            &t.id,
            UpdateTrigger {
                max_concurrent: Some(Some(5)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            get(&db, &t.id).await.unwrap().unwrap().max_concurrent,
            Some(5)
        );

        // An unrelated edit (None) leaves it untouched.
        update(
            &db,
            &t.id,
            UpdateTrigger {
                input_template: Some("changed".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            get(&db, &t.id).await.unwrap().unwrap().max_concurrent,
            Some(5)
        );

        // Some(None) clears it back to NULL.
        update(
            &db,
            &t.id,
            UpdateTrigger {
                max_concurrent: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(get(&db, &t.id).await.unwrap().unwrap().max_concurrent, None);
    }

    /// #239 migration: a `~/.pdo/pdo.db` created before `max_concurrent` existed
    /// must pick up the column on the next `init`, idempotently. `init`'s
    /// `CREATE TABLE IF NOT EXISTS` is a no-op against a pre-existing table, so
    /// the PRAGMA-guarded `ALTER` is the only path that migrates it. This builds
    /// the legacy schema by hand (the real failure mode `sqlite::memory:` after a
    /// plain `init` cannot reproduce, since a fresh DB already has the column).
    #[tokio::test]
    async fn init_is_idempotent_and_adds_max_concurrent_to_legacy_table() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Pre-#239 schema: the `triggers` table WITHOUT `max_concurrent`.
        sqlx::query(
            "CREATE TABLE triggers (
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
        .execute(&db)
        .await
        .unwrap();

        // A legacy row predating the column.
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, cron, created_at)
             VALUES ('trg-legacy', 'legacy', 'lib-pipe', '0 9 * * *', '2026-01-01T00:00:00.000Z')",
        )
        .execute(&db)
        .await
        .unwrap();

        // The column does not exist yet.
        let before = sqlx::query(
            "SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'",
        )
        .fetch_optional(&db)
        .await
        .unwrap();
        assert!(before.is_none(), "precondition: legacy table lacks the column");

        // init migrates it additively.
        init(&db).await.unwrap();

        let after = sqlx::query(
            "SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'",
        )
        .fetch_optional(&db)
        .await
        .unwrap();
        assert!(after.is_some(), "init must add the max_concurrent column");

        // The legacy row reads back with a NULL (unbounded) cap.
        let migrated = get(&db, "trg-legacy").await.unwrap().unwrap();
        assert_eq!(migrated.max_concurrent, None);

        // A second init is a no-op (the PRAGMA guard prevents a duplicate-column ALTER).
        init(&db).await.unwrap();
        assert_eq!(
            get(&db, "trg-legacy").await.unwrap().unwrap().max_concurrent,
            None
        );
    }

    // --- #244: guard-output capture on guard-exit-nonzero fire rows ---

    #[tokio::test]
    async fn guard_exit_nonzero_fire_persists_and_reads_back_guard_output() {
        let db = test_db().await;
        let t = create(&db, sample("guarded", "* * * * *")).await.unwrap();

        record_fire(
            &db,
            &t.id,
            &FireRecord {
                outcome: "guard-exit-nonzero".to_string(),
                reason: Some("guard exited non-zero".to_string()),
                run_id: None,
                guard_stdout: Some("checked 0 issues".to_string()),
                guard_stderr: Some("gh: no work to do\n".to_string()),
                guard_exit_code: Some(7),
            },
        )
        .await
        .unwrap();

        let history = fire_history(&db, &t.id).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].outcome, "guard-exit-nonzero");
        assert_eq!(history[0].guard_stdout.as_deref(), Some("checked 0 issues"));
        assert_eq!(
            history[0].guard_stderr.as_deref(),
            Some("gh: no work to do\n")
        );
        assert_eq!(history[0].guard_exit_code, Some(7));
    }

    #[tokio::test]
    async fn other_outcomes_leave_guard_output_null() {
        // A non-guard fire keeps the three columns NULL (D2 scoping).
        let db = test_db().await;
        let t = create(&db, sample("plain", "* * * * *")).await.unwrap();
        record_fire(
            &db,
            &t.id,
            &FireRecord {
                outcome: "fired".to_string(),
                reason: None,
                run_id: Some("20260606-100000-abc1234".to_string()),
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
            },
        )
        .await
        .unwrap();
        let history = fire_history(&db, &t.id).await.unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].guard_stdout.is_none());
        assert!(history[0].guard_stderr.is_none());
        assert!(history[0].guard_exit_code.is_none());
    }

    /// #244 migration: a `~/.pdo/pdo.db` whose `trigger_fires` predates the
    /// guard-output columns must pick them up on the next `init`, idempotently.
    /// Mirrors the `max_concurrent` legacy test but on `trigger_fires`. The
    /// silent-at-runtime trap is exactly here: miss the `ALTER` and prod
    /// INSERT/SELECT fail at request time, not compile time.
    #[tokio::test]
    async fn init_adds_guard_output_columns_to_legacy_trigger_fires_table() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Pre-#244 schema: `trigger_fires` WITHOUT the guard-output columns.
        sqlx::query(
            "CREATE TABLE trigger_fires (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trigger_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                outcome TEXT NOT NULL,
                reason TEXT,
                run_id TEXT
            )",
        )
        .execute(&db)
        .await
        .unwrap();

        // A legacy fire row predating the columns.
        sqlx::query(
            "INSERT INTO trigger_fires (trigger_id, ts, outcome, reason, run_id)
             VALUES ('trg-legacy', '2026-01-01T00:00:00.000Z', 'guard-exit-nonzero', 'no work', NULL)",
        )
        .execute(&db)
        .await
        .unwrap();

        // The columns do not exist yet.
        for col in ["guard_stdout", "guard_stderr", "guard_exit_code"] {
            let before =
                sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = ?")
                    .bind(col)
                    .fetch_optional(&db)
                    .await
                    .unwrap();
            assert!(before.is_none(), "precondition: legacy table lacks {col}");
        }

        // init migrates them additively.
        init(&db).await.unwrap();

        for col in ["guard_stdout", "guard_stderr", "guard_exit_code"] {
            let after =
                sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = ?")
                    .bind(col)
                    .fetch_optional(&db)
                    .await
                    .unwrap();
            assert!(after.is_some(), "init must add the {col} column");
        }

        // The legacy row reads back with NULL guard output.
        let history = fire_history(&db, "trg-legacy").await.unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].guard_stdout.is_none());
        assert!(history[0].guard_stderr.is_none());
        assert!(history[0].guard_exit_code.is_none());

        // A second init is a no-op (the PRAGMA guards prevent duplicate-column ALTERs).
        init(&db).await.unwrap();
        let history = fire_history(&db, "trg-legacy").await.unwrap();
        assert_eq!(history.len(), 1);
    }
}
