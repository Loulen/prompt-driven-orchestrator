//! Stage 3 (ADR-0080): the additive migrations of `instance_config` — a
//! database from before each column gains it on `init`, keeping what it held.

use pdo_settings::adapters::sqlite::init;
use pdo_settings::adapters::sqlite::instance_config::{
    get, set_triggers_paused, triggers_paused, update,
};
use pdo_settings::domain::instance_config::UpdateInstanceConfig;
use sqlx::SqlitePool;

use super::test_db;

#[tokio::test]
async fn init_migrates_pre_harness_schema() {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE instance_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            session_cap INTEGER,
            reaper_ttl_secs INTEGER,
            guard_timeout_secs INTEGER,
            default_model TEXT,
            triggers_paused INTEGER,
            default_sandbox TEXT,
            autocomplete_turn_end INTEGER,
            default_auto_name INTEGER,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, 'seed')")
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap();

    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.session_cap, Some(7), "pre-existing knob survives");
    assert_eq!(cfg.default_harness, None, "new column defaults to NULL");
    assert!(cfg.default_harness_model.is_empty());

    let map: std::collections::BTreeMap<String, String> =
        [("claude".to_string(), "opus".to_string())]
            .into_iter()
            .collect();
    update(
        &db,
        UpdateInstanceConfig {
            default_harness: Some("opencode".to_string()),
            default_harness_model: Some(map.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.default_harness.as_deref(), Some("opencode"));
    assert_eq!(cfg.default_harness_model, map);
}

#[tokio::test]
async fn init_migrates_pre_agent_choice_schema() {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE instance_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            session_cap INTEGER,
            reaper_ttl_secs INTEGER,
            guard_timeout_secs INTEGER,
            default_model TEXT,
            triggers_paused INTEGER,
            default_sandbox TEXT,
            autocomplete_turn_end INTEGER,
            default_auto_name INTEGER,
            default_harness TEXT,
            default_harness_model TEXT,
            auto_fail INTEGER,
            libassist_idle_ttl_secs INTEGER,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, 'seed')")
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap();

    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.session_cap, Some(7), "pre-existing knob survives");
    assert_eq!(cfg.agent_choice, None, "new column defaults to NULL");

    let choice = pdo_harness::agent_choice::AgentChoice::Profile {
        profile_id: "reviewer".to_string(),
    };
    update(
        &db,
        UpdateInstanceConfig {
            agent_choice: Some(Some(choice.clone())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db).await.unwrap().agent_choice, Some(choice));
}

#[tokio::test]
async fn init_migrates_pre_default_sandbox_schema() {
    // Installs created before #410 lack the `default_sandbox` column. Simulate the
    // pre-#410 schema (also missing default_model + triggers_paused), then prove `init`
    // adds the column idempotently and it is writable afterwards, without clobbering the
    // stored knob. It is the LAST guarded ALTER left: #471 retired the two sandbox-image
    // columns, so a fresh table never grows them and an old one keeps them, inert.
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
    sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 13, ?)")
        .bind(pdo_kernel::clock::now_iso())
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let cfg = get(&db).await.unwrap();
    assert_eq!(
        cfg.session_cap,
        Some(13),
        "existing knob must survive the ALTER"
    );
    assert_eq!(cfg.default_sandbox, None, "new column defaults to NULL");

    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_sandbox: Some("minimal".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_sandbox.as_deref(), Some("minimal"));
}

#[tokio::test]
async fn init_migrates_pre_autocomplete_turn_end_schema() {
    // Installs created before #469 lack the column. Simulate that schema, then
    // prove `init` adds it idempotently, the existing knob survives, and the
    // feature defaults to OFF on an upgraded install (ADR-0012).
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
    sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 19, ?)")
        .bind(pdo_kernel::clock::now_iso())
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let cfg = get(&db).await.unwrap();
    assert_eq!(
        cfg.session_cap,
        Some(19),
        "existing knob must survive the ALTER"
    );
    assert_eq!(
        cfg.autocomplete_turn_end, None,
        "an upgraded install must default to unset, i.e. OFF"
    );
    assert!(
        !pdo_run::application::stale::detector::autocomplete_turn_end_with(
            cfg.autocomplete_turn_end
        ) || pdo_run::application::stale::detector::env_autocomplete_turn_end() == Some(true),
        "resolved OFF unless the env tier says otherwise"
    );

    let updated = update(
        &db,
        UpdateInstanceConfig {
            autocomplete_turn_end: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.autocomplete_turn_end, Some(1));
}

#[tokio::test]
async fn init_migrates_pre_default_auto_name_schema() {
    // Installs created before #338 lack the column. Simulate that schema, then
    // prove `init` adds it idempotently, the existing knob survives, and an
    // upgraded install resolves to the pre-#338 behaviour (auto-name ON) unless
    // the env tier says otherwise.
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
    sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 21, ?)")
        .bind(pdo_kernel::clock::now_iso())
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap(); // idempotent

    let cfg = get(&db).await.unwrap();
    assert_eq!(
        cfg.session_cap,
        Some(21),
        "existing knob must survive the ALTER"
    );
    assert_eq!(
        cfg.default_auto_name, None,
        "an upgraded install must default to unset, i.e. fall through to env/default"
    );
    assert!(
        pdo_run::application::node::prompt::default_auto_name_with(cfg.default_auto_name)
            || pdo_run::application::node::prompt::env_default_auto_name() == Some(false),
        "resolved ON (pre-#338 behaviour) unless the env tier says otherwise"
    );

    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_auto_name: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_auto_name, Some(0));
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
    init(&db).await.unwrap();
    assert_eq!(get(&db).await.unwrap().session_cap, Some(7));
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
        .bind(pdo_kernel::clock::now_iso())
        .execute(&db)
        .await
        .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap();

    let cfg = get(&db).await.unwrap();
    assert_eq!(
        cfg.session_cap,
        Some(7),
        "existing knob must survive the ALTER"
    );
    assert_eq!(cfg.default_model, None, "new column defaults to NULL");

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
        .bind(pdo_kernel::clock::now_iso())
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
