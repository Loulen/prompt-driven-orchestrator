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
    /// Stored instance-wide default Claude model, or `None` when unset (#347).
    /// A work node with no `model:` override launches with this model; `None`
    /// falls through to the env seam ([`crate::tmux_session_manager::DEFAULT_MODEL_ENV`])
    /// then the account default (no `--model`). Free-text pass-through, no enum
    /// (ADR-0001): an invalid id fails loud in `claude`.
    pub default_model: Option<String>,
    /// RFC3339-millis UTC timestamp of the last write (or the seed).
    pub updated_at: String,
}

/// A partial edit. A `None` field leaves the stored value untouched.
///
/// The three numeric knobs are a set-only MVP: there is no path to clear a
/// stored value back to unset (a richer double-`Option` form was considered and
/// deferred — ADR-0015). `default_model` is the first *clear-capable* column
/// (#347): it stays a plain `Option<String>`, but `Some("")` is the clear
/// sentinel — [`update`] normalises an empty string to SQL `NULL` (a `Some("")`
/// must never persist, or it would win precedence and shadow env/account
/// default). This keeps the double-`Option` still deferred while giving the UI
/// a "Default" (unset) affordance.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateInstanceConfig {
    pub session_cap: Option<i64>,
    pub reaper_ttl_secs: Option<i64>,
    pub guard_timeout_secs: Option<i64>,
    pub default_model: Option<String>,
}

impl UpdateInstanceConfig {
    fn is_empty(&self) -> bool {
        self.session_cap.is_none()
            && self.reaper_ttl_secs.is_none()
            && self.guard_timeout_secs.is_none()
            && self.default_model.is_none()
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
            default_model      TEXT,
            triggers_paused    INTEGER,
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

    // Additive migration for pre-#347 databases: the `default_model` column is
    // absent on tables created before it joined the `CREATE TABLE` above. Add it
    // idempotently, PRAGMA-guarded (the first `ALTER` this module has needed;
    // precedent: `trigger_store` `max_concurrent`, #239). Never a migration
    // runner — a single guarded `ADD COLUMN`, safe on every boot.
    let has_default_model = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_model'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_model {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_model TEXT")
            .execute(db)
            .await?;
    }

    // Additive migration for pre-#348 databases: the `triggers_paused` column is
    // absent on tables created before the global trigger kill-switch. Same guarded
    // `ADD COLUMN` idiom as `default_model` above — safe on every boot.
    let has_triggers_paused = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'triggers_paused'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_triggers_paused {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN triggers_paused INTEGER")
            .execute(db)
            .await?;
    }

    Ok(())
}

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> InstanceConfig {
    InstanceConfig {
        session_cap: row.get("session_cap"),
        reaper_ttl_secs: row.get("reaper_ttl_secs"),
        guard_timeout_secs: row.get("guard_timeout_secs"),
        default_model: row.get("default_model"),
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
    if edit.default_model.is_some() {
        sets.push("default_model = ?");
    }
    // Always bump the write timestamp on a real edit.
    sets.push("updated_at = ?");

    let sql = format!(
        "UPDATE instance_config SET {} WHERE id = 1",
        sets.join(", ")
    );
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
    if let Some(v) = edit.default_model {
        // "" is the clear sentinel → bind SQL NULL (#347). A `Some("")` must
        // never persist: it would win the `stored → env → default` precedence
        // and both shadow the env/account default and reach the tail as an
        // empty `--model` value. Storing NULL restores fall-through.
        query = query.bind(if v.is_empty() { None } else { Some(v) });
    }
    query = query.bind(crate::event_log::now_iso());
    query.execute(db).await?;

    get(db).await
}

/// Read the global Trigger pause flag (#348). `NULL`/`0` ≡ not paused, `1` ≡
/// paused.
///
/// This flag is a **daemon-wide scheduler gate**, deliberately kept OUTSIDE the
/// `stored → env → default` `/settings` machinery ([`InstanceConfig`],
/// [`UpdateInstanceConfig`], `build_settings_view`, `put_settings`): a boolean
/// kill-switch has no env tier and no meaningful default other than "off", so
/// the per-field settings view would only add noise. It rides the same
/// singleton `instance_config` row purely for storage — the first knob on that
/// table excluded from the settings resolver.
pub async fn triggers_paused(db: &SqlitePool) -> Result<bool, sqlx::Error> {
    let v: Option<i64> =
        sqlx::query_scalar("SELECT triggers_paused FROM instance_config WHERE id = 1")
            .fetch_optional(db)
            .await?
            .flatten();
    Ok(v == Some(1))
}

/// Set (or clear) the global Trigger pause flag (#348). Unpausing stores SQL
/// `NULL` (≡ not paused) rather than `0`, keeping the "unset ≡ off" convention
/// the rest of the table follows. Bumps `updated_at` like [`update`].
pub async fn set_triggers_paused(db: &SqlitePool, paused: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE instance_config SET triggers_paused = ?, updated_at = ? WHERE id = 1")
        .bind(if paused { Some(1_i64) } else { None })
        .bind(crate::event_log::now_iso())
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
        assert_eq!(cfg.default_model, None);
        assert!(!cfg.updated_at.is_empty(), "seed must stamp updated_at");
    }

    #[tokio::test]
    async fn update_sets_default_model() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("opus"));
        // A re-read confirms persistence (not just the returned row).
        assert_eq!(
            get(&db).await.unwrap().default_model.as_deref(),
            Some("opus")
        );
        // The numeric knobs stay untouched.
        assert_eq!(updated.session_cap, None);
    }

    #[tokio::test]
    async fn update_clears_default_model_on_empty_string() {
        // "" is the clear sentinel: it must reset the column to NULL, not persist
        // an empty string (which would win precedence and reach the tail as an
        // empty `--model`).
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let cleared = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            cleared.default_model, None,
            "empty string must clear the stored model back to NULL"
        );
    }

    #[tokio::test]
    async fn default_model_only_edit_is_not_a_noop() {
        // Guard-rail for the `is_empty()` addition: an edit that touches ONLY
        // default_model (all numeric knobs None) must still write. Without the
        // `default_model.is_none()` clause it would fall into the no-op branch
        // and silently never persist.
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("haiku".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("haiku"));
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

    #[tokio::test]
    async fn init_migrates_pre_default_model_schema() {
        // Existing installs created the table before #347, so it lacks the
        // `default_model` column. Simulate that old schema, then prove `init`'s
        // PRAGMA-guarded ALTER adds the column idempotently and preserves the
        // stored numeric knobs (no data loss).
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Old CREATE (no default_model) + a seeded, non-default row.
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        // First init adds the column; a second is a no-op (guard holds).
        init(&db).await.unwrap();
        init(&db).await.unwrap();

        let cfg = get(&db).await.unwrap();
        assert_eq!(
            cfg.session_cap,
            Some(7),
            "existing knob must survive the ALTER"
        );
        assert_eq!(cfg.default_model, None, "new column defaults to NULL");

        // And the migrated column is writable.
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("opus"));
    }

    #[tokio::test]
    async fn triggers_paused_defaults_off() {
        // A fresh row has NULL triggers_paused ≡ not paused (#348).
        let db = test_db().await;
        assert!(!triggers_paused(&db).await.unwrap());
    }

    #[tokio::test]
    async fn set_triggers_paused_round_trips_and_clears_to_null() {
        // #348: pausing stores 1, unpausing stores NULL (not 0), and both are
        // observable through `triggers_paused`.
        let db = test_db().await;

        set_triggers_paused(&db, true).await.unwrap();
        assert!(triggers_paused(&db).await.unwrap(), "paused must read true");
        // Unpause stores SQL NULL, keeping the "unset ≡ off" table convention.
        set_triggers_paused(&db, false).await.unwrap();
        assert!(
            !triggers_paused(&db).await.unwrap(),
            "unpaused must read false"
        );
        let raw: Option<i64> =
            sqlx::query_scalar("SELECT triggers_paused FROM instance_config WHERE id = 1")
                .fetch_optional(&db)
                .await
                .unwrap()
                .flatten();
        assert_eq!(raw, None, "unpause must persist NULL, never 0");
    }

    #[tokio::test]
    async fn triggers_paused_survives_init_and_is_orthogonal_to_settings() {
        // The kill-switch persists across a daemon restart (a second `init`) and
        // does not disturb the `/settings` knobs, nor they it (#348: two
        // orthogonal channels sharing one row).
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        set_triggers_paused(&db, true).await.unwrap();

        init(&db).await.unwrap(); // mimic a restart

        assert!(
            triggers_paused(&db).await.unwrap(),
            "pause flag must survive a restart"
        );
        assert_eq!(
            get(&db).await.unwrap().session_cap,
            Some(5),
            "a settings knob must survive the pause write untouched"
        );
    }

    #[tokio::test]
    async fn init_migrates_pre_triggers_paused_schema() {
        // Installs created before #348 lack the `triggers_paused` column. Simulate
        // that schema (also missing default_model, so this covers the double
        // guarded ALTER), then prove `init` adds the column idempotently and the
        // flag is writable afterwards, without clobbering the stored knob.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 9, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap(); // idempotent

        assert!(
            !triggers_paused(&db).await.unwrap(),
            "migrated column defaults to not-paused"
        );
        assert_eq!(
            get(&db).await.unwrap().session_cap,
            Some(9),
            "existing knob must survive the ALTER"
        );
        set_triggers_paused(&db, true).await.unwrap();
        assert!(
            triggers_paused(&db).await.unwrap(),
            "the migrated column is writable"
        );
    }
}
