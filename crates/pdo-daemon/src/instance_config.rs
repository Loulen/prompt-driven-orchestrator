//! Instance-wide configuration store (#129, ADR-0015).
//!
//! Three daemon-wide runtime knobs — the concurrent-session cap, the tmux reaper
//! TTL, and the Trigger guard timeout — historically lived only in environment
//! variables (shell + restart). This module is their durable home: a SQLite
//! *singleton* row (`id = 1`) in `pdo.db`, mirroring [`crate::trigger_store`]
//! (config + mutable state, not a canvas-backed artifact → wrong fit for YAML,
//! cf. CONTEXT.md *Persistence — table SQLite*).
//!
//! **Load-bearing:** every column is NULLABLE and the seed row is all-NULL. A
//! NULL stored value means "unset" and the resolver falls through to the env
//! var, then the code default — the `stored → env → default` precedence of
//! ADR-0015. Keeping the columns NULLABLE (rather than `NOT NULL DEFAULT 20`) is
//! deliberate: a non-null default would make the stored tier always win, so the
//! env would never be consulted and the existing cap/TTL tests (which set env,
//! expect env) would break.
//!
//! The public API mirrors the store idiom: `init` (create + seed), `get`
//! (singleton fetch), and `update` (partial, set-only edit). Precedence itself
//! lives with each reader (`admission::configured_cap_with`,
//! `tmux_session_manager::reaper_ttl_with`, `guard_runner::guard_timeout_with`);
//! this module only persists the `stored` tier.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// The persisted singleton config row. Each field is `None` when unset (the
/// resolver then falls through to env → default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// Stored global session cap, or `None` when unset.
    pub session_cap: Option<i64>,
    /// Stored tmux reaper TTL in seconds, or `None` when unset.
    pub reaper_ttl_secs: Option<i64>,
    /// Stored Trigger guard timeout in **seconds**, or `None` when unset. Note
    /// the env seam ([`crate::guard_runner::GUARD_TIMEOUT_MS_OVERRIDE_ENV`]) is
    /// in milliseconds; the stored value is seconds (ADR-0015).
    pub guard_timeout_secs: Option<i64>,
    /// RFC3339-millis UTC timestamp of the last write (or the seed).
    pub updated_at: String,
}

/// A partial edit. A `None` field leaves the stored value untouched. This is a
/// set-only MVP: there is no path to clear a stored value back to unset (a
/// richer double-`Option` form was considered and deferred — ADR-0015).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateInstanceConfig {
    pub session_cap: Option<i64>,
    pub reaper_ttl_secs: Option<i64>,
    pub guard_timeout_secs: Option<i64>,
}

impl UpdateInstanceConfig {
    fn is_empty(&self) -> bool {
        self.session_cap.is_none()
            && self.reaper_ttl_secs.is_none()
            && self.guard_timeout_secs.is_none()
    }
}

/// Create the `instance_config` table if absent and seed the all-NULL singleton
/// row. Idempotent: safe to call on every boot.
///
/// Future knobs are added via an idempotent PRAGMA-guarded
/// `ALTER TABLE … ADD COLUMN` (precedent: `max_concurrent` #239 in
/// `trigger_store`), never a migration runner.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS instance_config (
            id                 INTEGER PRIMARY KEY CHECK (id = 1),
            session_cap        INTEGER,
            reaper_ttl_secs    INTEGER,
            guard_timeout_secs INTEGER,
            updated_at         TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Seed the singleton row with all-NULL knobs. `INSERT OR IGNORE` is a no-op
    // once the row exists, so this cannot clobber a stored config on a restart.
    let now = crate::event_log::now_iso();
    sqlx::query("INSERT OR IGNORE INTO instance_config (id, updated_at) VALUES (1, ?)")
        .bind(&now)
        .execute(db)
        .await?;

    Ok(())
}

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> InstanceConfig {
    InstanceConfig {
        session_cap: row.get("session_cap"),
        reaper_ttl_secs: row.get("reaper_ttl_secs"),
        guard_timeout_secs: row.get("guard_timeout_secs"),
        updated_at: row.get("updated_at"),
    }
}

/// Fetch the singleton config row. Always present after [`init`].
pub async fn get(db: &SqlitePool) -> Result<InstanceConfig, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM instance_config WHERE id = 1")
        .fetch_optional(db)
        .await?;
    row.as_ref()
        .map(row_to_config)
        .ok_or(sqlx::Error::RowNotFound)
}

/// Apply a partial config edit and return the updated row. A no-op edit (no
/// field set) returns the current row unchanged and does not bump `updated_at`.
pub async fn update(
    db: &SqlitePool,
    edit: UpdateInstanceConfig,
) -> Result<InstanceConfig, sqlx::Error> {
    if edit.is_empty() {
        return get(db).await;
    }

    // Build the SET clause field-by-field, then bind in the same order so the
    // positional placeholders line up (mirrors trigger_store::update).
    let mut sets: Vec<&str> = Vec::new();
    if edit.session_cap.is_some() {
        sets.push("session_cap = ?");
    }
    if edit.reaper_ttl_secs.is_some() {
        sets.push("reaper_ttl_secs = ?");
    }
    if edit.guard_timeout_secs.is_some() {
        sets.push("guard_timeout_secs = ?");
    }
    // Always bump the write timestamp on a real edit.
    sets.push("updated_at = ?");

    let sql = format!("UPDATE instance_config SET {} WHERE id = 1", sets.join(", "));
    let mut query = sqlx::query(&sql);
    if let Some(v) = edit.session_cap {
        query = query.bind(v);
    }
    if let Some(v) = edit.reaper_ttl_secs {
        query = query.bind(v);
    }
    if let Some(v) = edit.guard_timeout_secs {
        query = query.bind(v);
    }
    query = query.bind(crate::event_log::now_iso());
    query.execute(db).await?;

    get(db).await
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

    #[tokio::test]
    async fn fresh_row_is_all_null() {
        // Load-bearing (ADR-0015): a fresh install seeds an all-NULL row so the
        // resolver falls through to env → default. A non-null default here would
        // silently break the env-driven cap/TTL tests.
        let db = test_db().await;
        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.session_cap, None);
        assert_eq!(cfg.reaper_ttl_secs, None);
        assert_eq!(cfg.guard_timeout_secs, None);
        assert!(!cfg.updated_at.is_empty(), "seed must stamp updated_at");
    }

    #[tokio::test]
    async fn init_is_idempotent_and_never_clobbers_stored() {
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(7),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // A second init (mimicking a daemon restart) must not reset the row.
        init(&db).await.unwrap();
        assert_eq!(get(&db).await.unwrap().session_cap, Some(7));
    }

    #[tokio::test]
    async fn update_sets_only_the_given_fields() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(4),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.session_cap, Some(4));
        // Untouched fields stay NULL.
        assert_eq!(updated.reaper_ttl_secs, None);
        assert_eq!(updated.guard_timeout_secs, None);

        // A second partial edit changes only its field, leaving cap intact.
        let updated = update(
            &db,
            UpdateInstanceConfig {
                reaper_ttl_secs: Some(120),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.session_cap, Some(4));
        assert_eq!(updated.reaper_ttl_secs, Some(120));
    }

    #[tokio::test]
    async fn update_bumps_updated_at() {
        let db = test_db().await;
        let before = get(&db).await.unwrap().updated_at;
        // now_iso() is millisecond-resolution; ensure a distinct instant.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let after = update(
            &db,
            UpdateInstanceConfig {
                guard_timeout_secs: Some(30),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_ne!(after.updated_at, before, "a real edit must bump updated_at");
    }

    #[tokio::test]
    async fn empty_update_is_a_noop_and_leaves_updated_at() {
        let db = test_db().await;
        let before = get(&db).await.unwrap();
        let after = update(&db, UpdateInstanceConfig::default()).await.unwrap();
        assert_eq!(after, before, "a no-op edit must not mutate the row");
    }

    #[tokio::test]
    async fn get_is_idempotent() {
        let db = test_db().await;
        assert_eq!(get(&db).await.unwrap(), get(&db).await.unwrap());
    }
}
