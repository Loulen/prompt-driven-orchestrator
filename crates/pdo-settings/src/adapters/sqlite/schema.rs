//! The settings context's tables: `instance_config`, the singleton row of the
//! stored tier, and `agent_profiles`, seeded with the reserved `Default`.
//! Idempotent: safe on every boot. Future knobs are added by an idempotent,
//! PRAGMA-guarded `ALTER TABLE … ADD COLUMN`, never a migration runner.

use pdo_harness::registry::CLAUDE;
use sqlx::SqlitePool;

use crate::domain::agent_profile::{DEFAULT_PROFILE_ID, DEFAULT_PROFILE_NAME};

/// The columns added after the table first shipped, in the order they joined
/// it, with the issue that added each. Every one is NULLABLE, never `NOT NULL
/// DEFAULT`: a non-null default would make the stored tier always win and
/// silence the env tier.
const ADDED_COLUMNS: &[(&str, &str)] = &[
    // #347: the instance default model.
    ("default_model", "TEXT"),
    // #348: the global Trigger kill-switch.
    ("triggers_paused", "INTEGER"),
    // #410: NULL keeps the `off`-by-default (byte-identical) floor.
    ("default_sandbox", "TEXT"),
    // #469: NULL keeps an existing install OFF (ADR-0012), the env still able to speak.
    ("autocomplete_turn_end", "INTEGER"),
    // #338: NULL preserves the pre-#338 behaviour (the manager names a nameless Run).
    ("default_auto_name", "INTEGER"),
    // #550: the multi-harness axis.
    ("default_harness", "TEXT"),
    ("default_harness_model", "TEXT"),
    // ADR-0049.
    ("auto_fail", "INTEGER"),
    // #594: the shared, idle-reaped library assistant.
    ("libassist_idle_ttl_secs", "INTEGER"),
    // #563: NULL falls through to the legacy `default_harness`/`default_harness_model`.
    ("agent_choice", "TEXT"),
    // #669: the Instance tier's skills selection; NULL selects nothing.
    ("skills", "TEXT"),
    // #697: NULL keeps the version check ON, a stored `0` turns it off.
    ("update_check", "INTEGER"),
    // #751: NULL keeps the built-in default (the human resolves, the agent proposes).
    ("review_agent_can_resolve", "INTEGER"),
    // Manager on demand: NULL so a stored `0` stays a decision that beats the env.
    ("manager_enabled", "INTEGER"),
    // Manager on demand: the agentic-profile pin; NULL reads « Follow the Run ».
    ("manager_profile", "TEXT"),
    // #779: the per-run attachment budget.
    ("max_attachments_mb", "INTEGER"),
    // ADR-0090: NULL keeps the OTLP export off, the env still able to speak.
    ("telemetry_otlp", "INTEGER"),
    ("telemetry_otlp_endpoint", "TEXT"),
    ("telemetry_otlp_headers", "TEXT"),
    ("telemetry_prometheus", "INTEGER"),
    ("telemetry_include_actor", "INTEGER"),
    // The address humans open PDO at; NULL keeps the env tier, then none.
    ("public_url", "TEXT"),
    // CORE-97: where a repository given by its URL is cloned; NULL → ~/pdo/repos.
    ("clones_folder", "TEXT"),
];

/// Create both tables if absent, seed the all-NULL singleton row and the
/// reserved profile, and add the columns an older database lacks.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    init_instance_config(db).await?;
    init_agent_profiles(db).await
}

async fn init_instance_config(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS instance_config (
            id                 INTEGER PRIMARY KEY CHECK (id = 1),
            session_cap        INTEGER,
            reaper_ttl_secs    INTEGER,
            guard_timeout_secs INTEGER,
            default_model      TEXT,
            triggers_paused    INTEGER,
            default_sandbox    TEXT,
            autocomplete_turn_end INTEGER,
            default_auto_name  INTEGER,
            default_harness    TEXT,
            default_harness_model TEXT,
            auto_fail          INTEGER,
            libassist_idle_ttl_secs INTEGER,
            agent_choice       TEXT,
            update_check       INTEGER,
            review_agent_can_resolve INTEGER,
            manager_enabled    INTEGER,
            manager_profile    TEXT,
            updated_at         TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // `INSERT OR IGNORE` is a no-op once the row exists, so this cannot clobber
    // a stored config on a restart.
    sqlx::query("INSERT OR IGNORE INTO instance_config (id, updated_at) VALUES (1, ?)")
        .bind(pdo_kernel::clock::now_iso())
        .execute(db)
        .await?;

    for (column, kind) in ADDED_COLUMNS {
        if !has_column(db, column).await? {
            // The name and the type come from the constant above, never from a
            // request.
            sqlx::query(&format!(
                "ALTER TABLE instance_config ADD COLUMN {column} {kind}"
            ))
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

/// Whether `instance_config` has `column`.
pub(super) async fn has_column(db: &SqlitePool, column: &str) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("SELECT 1 FROM pragma_table_info('instance_config') WHERE name = ?")
            .bind(column)
            .fetch_optional(db)
            .await?
            .is_some(),
    )
}

/// Unlike most tables, this one IS seeded: the reserved `Default` profile must
/// exist on every instance, immediately.
async fn init_agent_profiles(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_profiles (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT NOT NULL,
            model      TEXT,
            effort     TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    let exists = sqlx::query("SELECT 1 FROM agent_profiles WHERE id = ?")
        .bind(DEFAULT_PROFILE_ID)
        .fetch_optional(db)
        .await?
        .is_some();
    if !exists {
        let now = pdo_kernel::clock::now_iso();
        sqlx::query(
            "INSERT INTO agent_profiles (id, name, harness, model, effort, created_at, updated_at) \
             VALUES (?, ?, ?, NULL, NULL, ?, ?)",
        )
        .bind(DEFAULT_PROFILE_ID)
        .bind(DEFAULT_PROFILE_NAME)
        .bind(CLAUDE)
        .bind(&now)
        .bind(&now)
        .execute(db)
        .await?;
    }
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_profiles_name_nocase \
         ON agent_profiles(name COLLATE NOCASE)",
    )
    .execute(db)
    .await?;
    Ok(())
}
