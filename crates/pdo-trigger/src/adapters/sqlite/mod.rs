//! The Triggers and their fire history over SQLite (`triggers` +
//! `trigger_fires`), in the daemon's `pdo.db`.
//!
//! `update` folds every field edit, including the enable/disable bit, into ONE
//! atomic write.

mod rows;
mod schema;
mod update;

use pdo_skill::domain::selection;
use sqlx::SqlitePool;

use crate::application::ports::{BoxFuture, StorageError, Stored, TriggerRepository};
use crate::domain::trigger::{FireRecord, NewTrigger, Trigger, TriggerFire, UpdateTrigger};
use rows::{row_to_fire, row_to_trigger};
pub use schema::init;
pub use update::update;

/// The Triggers of the daemon's database.
#[derive(Clone)]
pub struct SqliteTriggers {
    db: SqlitePool,
}

impl SqliteTriggers {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn stored<T>(result: Result<T, sqlx::Error>) -> Stored<T> {
    result.map_err(|e| StorageError(e.to_string()))
}

impl TriggerRepository for SqliteTriggers {
    fn init(&self) -> BoxFuture<'_, Stored<()>> {
        Box::pin(async move { stored(init(&self.db).await) })
    }

    fn create(&self, new: NewTrigger) -> BoxFuture<'_, Stored<Trigger>> {
        Box::pin(async move { stored(create(&self.db, new).await) })
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<Trigger>>> {
        Box::pin(async move { stored(get(&self.db, id).await) })
    }

    fn list(&self) -> BoxFuture<'_, Stored<Vec<Trigger>>> {
        Box::pin(async move { stored(list(&self.db).await) })
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>> {
        Box::pin(async move { stored(delete(&self.db, id).await) })
    }

    fn update<'a>(&'a self, id: &'a str, edit: UpdateTrigger) -> BoxFuture<'a, Stored<()>> {
        Box::pin(async move { stored(update(&self.db, id, edit).await) })
    }

    fn due<'a>(&'a self, now: &'a str) -> BoxFuture<'a, Stored<Vec<Trigger>>> {
        Box::pin(async move { stored(due_triggers(&self.db, now).await) })
    }

    fn set_next_fire<'a>(
        &'a self,
        id: &'a str,
        next_fire_at: Option<&'a str>,
    ) -> BoxFuture<'a, Stored<()>> {
        Box::pin(async move { stored(set_next_fire(&self.db, id, next_fire_at).await) })
    }

    fn record_fire<'a>(&'a self, id: &'a str, record: &'a FireRecord) -> BoxFuture<'a, Stored<()>> {
        Box::pin(async move { stored(record_fire(&self.db, id, record).await) })
    }

    fn fires<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Vec<TriggerFire>>> {
        Box::pin(async move { stored(fire_history(&self.db, id).await) })
    }
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
    let now = pdo_kernel::clock::now_iso();
    let variables_str = serde_json::to_string(&new.variables).unwrap_or_else(|_| "{}".to_string());

    sqlx::query(
        "INSERT INTO triggers
            (id, name, pipeline_id, pipeline_name, target_repo, target_repos, source_branch,
             input_template, variables, cron, guard_command, overlap_policy,
             max_concurrent, sandbox, harness, agent_choice, skills, auto_name, enabled, next_fire_at, last_fired_at, last_outcome, created_at, last_editor)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, NULL, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(&new.name)
    .bind(&new.pipeline_id)
    .bind(&new.pipeline_name)
    .bind(&new.target_repo)
    .bind(&new.target_repos)
    .bind(&new.source_branch)
    .bind(&new.input_template)
    .bind(&variables_str)
    .bind(&new.cron)
    .bind(&new.guard_command)
    .bind(&new.overlap_policy)
    .bind(new.max_concurrent)
    .bind(&new.sandbox)
    .bind(&new.harness)
    .bind(
        new.agent_choice
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default()),
    )
    .bind(selection::to_stored_json(
        &selection::normalise(new.skills.clone()),
    ))
    .bind(if new.auto_name { 1_i64 } else { 0_i64 })
    .bind(&new.next_fire_at)
    .bind(&now)
    .bind(stored_actor(new.last_editor.as_ref()))
    .execute(db)
    .await?;

    get(db, &id).await?.ok_or(sqlx::Error::RowNotFound)
}

/// An Actor as its column stores it: JSON text, NULL for none.
pub(crate) fn stored_actor(actor: Option<&pdo_kernel::Actor>) -> Option<String> {
    actor.and_then(|a| serde_json::to_string(a).ok())
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

/// Enabled Triggers whose `next_fire_at` is at or before `now` — the scheduler's
/// central query.
///
/// Comparison and ordering are **timezone-normalised** via `julianday()` rather
/// than a raw string compare: `next_fire_at` is invariably canonical UTC, so a
/// string compare *would* be correct, but a stray local-offset row (legacy data,
/// or `chrono::Local::now()` slipping back into a writer) sorts lexicographically
/// *after* a `…Z` now-string and would silently go dormant for hours.
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
    let ts = pdo_kernel::clock::now_iso();
    sqlx::query(
        "INSERT INTO trigger_fires
            (trigger_id, ts, outcome, reason, run_id,
             guard_stdout, guard_stderr, guard_exit_code, source)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(trigger_id)
    .bind(&ts)
    .bind(&record.outcome)
    .bind(&record.reason)
    .bind(&record.run_id)
    .bind(&record.guard_stdout)
    .bind(&record.guard_stderr)
    .bind(record.guard_exit_code)
    .bind(&record.source)
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
    // Capped: the guard-output blobs make each row heavy and a minute-cron
    // trigger accrues ~1440 rows/day, while the panel only shows the recent tail.
    let rows = sqlx::query(
        "SELECT id, trigger_id, ts, outcome, reason, run_id,
                guard_stdout, guard_stderr, guard_exit_code, source
         FROM trigger_fires WHERE trigger_id = ? ORDER BY id DESC LIMIT 200",
    )
    .bind(trigger_id)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_fire).collect())
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
