//! The schema of the Triggers (`triggers`, `trigger_fires`) and its additive
//! migrations.

use sqlx::SqlitePool;

/// Create the `triggers` and `trigger_fires` tables if they do not exist.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS triggers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            pipeline_id TEXT NOT NULL,
            pipeline_name TEXT NOT NULL DEFAULT '',
            target_repo TEXT,
            target_repos TEXT,
            source_branch TEXT,
            input_template TEXT NOT NULL DEFAULT '',
            variables JSON NOT NULL DEFAULT '{}',
            cron TEXT NOT NULL,
            guard_command TEXT,
            overlap_policy TEXT NOT NULL DEFAULT 'skip',
            max_concurrent INTEGER,
            sandbox TEXT,
            harness TEXT,
            agent_choice TEXT,
            auto_name INTEGER NOT NULL DEFAULT 1,
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
            guard_exit_code INTEGER,
            source TEXT
        )",
    )
    .execute(db)
    .await?;

    // Additive migrations. A `~/.pdo/pdo.db` predating a column got the table via
    // `CREATE TABLE IF NOT EXISTS` above, a no-op there, so the column must be
    // added out-of-band; there is no migration runner. The PRAGMA guard keeps the
    // ALTER idempotent (a bare one errors "duplicate column name") and is
    // preferred over swallowing its error, which would hide genuine failures.
    let has_col =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_col {
        sqlx::query("ALTER TABLE triggers ADD COLUMN max_concurrent INTEGER")
            .execute(db)
            .await?;
    }

    // `sandbox` — NULLABLE: a NULL row inherits the instance default.
    let has_sandbox =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'sandbox'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_sandbox {
        sqlx::query("ALTER TABLE triggers ADD COLUMN sandbox TEXT")
            .execute(db)
            .await?;
    }

    // `harness` — NULLABLE: a NULL row inherits the instance default at fire time.
    let has_harness =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'harness'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_harness {
        sqlx::query("ALTER TABLE triggers ADD COLUMN harness TEXT")
            .execute(db)
            .await?;
    }

    // `agent_choice` — NULLABLE: a NULL row inherits the legacy `harness` tier
    // (and the tiers below it) at fire time.
    let has_agent_choice =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'agent_choice'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_agent_choice {
        sqlx::query("ALTER TABLE triggers ADD COLUMN agent_choice TEXT")
            .execute(db)
            .await?;
    }

    // `skills` — NULLABLE (#669): a NULL row adds no skill at the Run tier.
    let has_skills =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'skills'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_skills {
        sqlx::query("ALTER TABLE triggers ADD COLUMN skills TEXT")
            .execute(db)
            .await?;
    }

    // `target_repos` — NULLABLE: a NULL row fires mono-repo.
    let has_target_repos =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'target_repos'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_target_repos {
        sqlx::query("ALTER TABLE triggers ADD COLUMN target_repos TEXT")
            .execute(db)
            .await?;
    }

    // `auto_name` — `NOT NULL DEFAULT 1`, unlike the nullable columns: a plain
    // flat bool (mirror of `enabled`). The default `1` is load-bearing — every
    // pre-existing Trigger keeps auto-naming its Runs.
    let has_auto_name =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'auto_name'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_auto_name {
        sqlx::query("ALTER TABLE triggers ADD COLUMN auto_name INTEGER NOT NULL DEFAULT 1")
            .execute(db)
            .await?;
    }

    // Guard-output columns on `trigger_fires`; without them the runtime
    // INSERT/SELECT below would fail.
    for (col, ddl) in [
        (
            "guard_stdout",
            "ALTER TABLE trigger_fires ADD COLUMN guard_stdout TEXT",
        ),
        (
            "guard_stderr",
            "ALTER TABLE trigger_fires ADD COLUMN guard_stderr TEXT",
        ),
        (
            "guard_exit_code",
            "ALTER TABLE trigger_fires ADD COLUMN guard_exit_code INTEGER",
        ),
        ("source", "ALTER TABLE trigger_fires ADD COLUMN source TEXT"),
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

    // Back `GET /stats/overview`'s fires-per-period query and its
    // fires-per-pipeline `LEFT JOIN triggers`. `CREATE INDEX IF NOT EXISTS` is
    // natively idempotent, so it needs no PRAGMA guard.
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_trigger_fires_ts ON trigger_fires(ts)")
        .execute(db)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_trigger_fires_trigger ON trigger_fires(trigger_id)",
    )
    .execute(db)
    .await?;

    Ok(())
}
