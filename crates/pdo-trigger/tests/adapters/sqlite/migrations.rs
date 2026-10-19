use pdo_trigger::adapters::sqlite::*;

/// #563 migration: a `~/.pdo/pdo.db` created before `agent_choice` existed must
/// pick up the column on the next `init`, and a legacy row must read back with
/// `agent_choice = None` (the legacy `harness` field, if any, still decides) —
/// mirror of `init_adds_harness_to_legacy_table` above.
#[tokio::test]
async fn init_adds_agent_choice_to_legacy_table() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    // Pre-#563 schema: `triggers` WITH `harness` but WITHOUT `agent_choice`.
    sqlx::query(
        "CREATE TABLE triggers (
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
            auto_name INTEGER NOT NULL DEFAULT 1,
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
    sqlx::query(
        "INSERT INTO triggers (id, name, pipeline_id, cron, harness, created_at)
         VALUES ('trg-legacy', 'legacy', 'lib-pipe', '0 9 * * *', 'claude', '2026-01-01T00:00:00.000Z')",
    )
    .execute(&db)
    .await
    .unwrap();

    let before =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'agent_choice'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(
        before.is_none(),
        "precondition: legacy table lacks the column"
    );

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let after =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'agent_choice'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(after.is_some(), "init must add the column");

    let legacy = get(&db, "trg-legacy").await.unwrap().unwrap();
    assert_eq!(legacy.harness.as_deref(), Some("claude"));
    assert_eq!(legacy.agent_choice, None);
}

/// #551 migration: a `~/.pdo/pdo.db` created before `harness` existed must pick up the
/// column on the next `init`, and a legacy row must read back with `harness = None`
/// (inherit the instance default) — the pre-#551 behaviour.
#[tokio::test]
async fn init_adds_harness_to_legacy_table() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    // Pre-#551 schema: `triggers` WITHOUT `harness` (but WITH the earlier
    // sandbox/auto_name columns, so this exercises the tail ALTER).
    sqlx::query(
        "CREATE TABLE triggers (
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
            auto_name INTEGER NOT NULL DEFAULT 1,
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
    sqlx::query(
        "INSERT INTO triggers (id, name, pipeline_id, cron, created_at)
         VALUES ('trg-legacy', 'legacy', 'lib-pipe', '0 9 * * *', '2026-01-01T00:00:00.000Z')",
    )
    .execute(&db)
    .await
    .unwrap();

    let before = sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'harness'")
        .fetch_optional(&db)
        .await
        .unwrap();
    assert!(
        before.is_none(),
        "precondition: legacy table lacks the column"
    );

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let after = sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'harness'")
        .fetch_optional(&db)
        .await
        .unwrap();
    assert!(after.is_some(), "init must add the harness column");

    // The legacy row reads back inheriting (None), the pre-#551 behaviour.
    let migrated = get(&db, "trg-legacy").await.unwrap().unwrap();
    assert_eq!(migrated.harness, None);
}

/// #338 migration: a `~/.pdo/pdo.db` created before `auto_name` existed must pick up
/// the column on the next `init`, and every legacy row must read back as auto-name ON
/// (the pre-#338 behaviour) thanks to the `NOT NULL DEFAULT 1` column.
#[tokio::test]
async fn init_adds_auto_name_to_legacy_table_defaulting_on() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    // Pre-#338 schema: the `triggers` table WITHOUT `auto_name` (but WITH the
    // earlier `max_concurrent`/`sandbox` columns, so this exercises the tail ALTER).
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
            max_concurrent INTEGER,
            sandbox TEXT,
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
    sqlx::query(
        "INSERT INTO triggers (id, name, pipeline_id, cron, created_at)
         VALUES ('trg-legacy', 'legacy', 'lib-pipe', '0 9 * * *', '2026-01-01T00:00:00.000Z')",
    )
    .execute(&db)
    .await
    .unwrap();

    // The column does not exist yet.
    let before =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'auto_name'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(
        before.is_none(),
        "precondition: legacy table lacks the column"
    );

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let after = sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'auto_name'")
        .fetch_optional(&db)
        .await
        .unwrap();
    assert!(after.is_some(), "init must add the auto_name column");

    // The legacy row reads back auto-name ON — the pre-#338 behaviour is preserved.
    let migrated = get(&db, "trg-legacy").await.unwrap().unwrap();
    assert!(
        migrated.auto_name,
        "a pre-#338 Trigger must keep auto-naming its Runs"
    );
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
    let before =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(
        before.is_none(),
        "precondition: legacy table lacks the column"
    );

    // init migrates it additively.
    init(&db).await.unwrap();

    let after =
        sqlx::query("SELECT 1 FROM pragma_table_info('triggers') WHERE name = 'max_concurrent'")
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
        get(&db, "trg-legacy")
            .await
            .unwrap()
            .unwrap()
            .max_concurrent,
        None
    );
}

// --- #244: guard-output capture on guard-exit-nonzero fire rows ---

/// ADR-0087 migration: the `project_id` column is added once, and filled that
/// once with the Projet the rule before ADR-0087 gave the Trigger's Runs (the
/// first Projet holding its repository). A later change of the members moves
/// no Trigger.
#[tokio::test]
async fn init_records_the_projet_the_old_rule_gave_a_trigger_once() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE triggers (
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
    sqlx::query(
        "CREATE TABLE project_members (
            path TEXT PRIMARY KEY, project_id TEXT NOT NULL, created_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_members (path, project_id, created_at) VALUES ('/repos/web', 'p1', 's')",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO triggers (id, name, pipeline_id, target_repo, cron, created_at) VALUES
            ('trg-in', 'in', 'pipe', '/repos/web/', '0 9 * * *', 's'),
            ('trg-out', 'out', 'pipe', '/repos/loose', '0 9 * * *', 's')",
    )
    .execute(&db)
    .await
    .unwrap();

    init(&db).await.unwrap();
    assert_eq!(
        get(&db, "trg-in")
            .await
            .unwrap()
            .unwrap()
            .project_id
            .as_deref(),
        Some("p1")
    );
    assert_eq!(get(&db, "trg-out").await.unwrap().unwrap().project_id, None);

    // Once: a member added afterwards moves no Trigger.
    sqlx::query(
        "INSERT INTO project_members (path, project_id, created_at) VALUES ('/repos/loose', 'p2', 's')",
    )
    .execute(&db)
    .await
    .unwrap();
    init(&db).await.unwrap();
    assert_eq!(get(&db, "trg-out").await.unwrap().unwrap().project_id, None);
}
