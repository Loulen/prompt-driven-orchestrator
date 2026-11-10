//! Stage 3 (ADR-0080): the two values the row stores off the page — the
//! global Trigger pause, and whatever is left in the retired sandbox image
//! columns.

use pdo_sandbox::domain::retired_settings::RETIRED_SANDBOX_IMAGE_COLUMNS;
use pdo_settings::adapters::sqlite::init;
use pdo_settings::adapters::sqlite::instance_config::{get, update};
use pdo_settings::adapters::sqlite::instance_config::{
    retired_sandbox_values, set_triggers_paused, triggers_paused,
};
use pdo_settings::domain::instance_config::UpdateInstanceConfig;
use sqlx::SqlitePool;

use super::test_db;

/// A table created by TODAY's `init` has neither retired column — the reader must be silent,
/// not error. This is the reference instance's case, and the only one an untouched install ever
/// hits.
#[tokio::test]
async fn a_fresh_table_has_no_retired_sandbox_image_columns() {
    let db = test_db().await;
    let cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('instance_config')")
            .fetch_all(&db)
            .await
            .unwrap();
    for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        assert!(
            !cols.contains(&column.to_string()),
            "#471: a fresh table must not grow `{column}` back; got {cols:?}"
        );
    }
    assert_eq!(retired_sandbox_values(&db).await.unwrap(), Vec::new());
}

/// A pre-#471 install: the columns exist and one carries a value the user typed. It must be
/// FOUND — that is the whole point of AC5, a value that stops mattering never disappears in
/// silence — and it must not resurface anywhere in [`InstanceConfig`].
#[tokio::test]
async fn a_stored_retired_value_is_found_but_is_not_config_any_more() {
    let db = pre_471_db().await;
    sqlx::query(
        "UPDATE instance_config SET image_source = 'dockerfile', \
         dockerfile_path = '/repo/docker/Dockerfile' WHERE id = 1",
    )
    .execute(&db)
    .await
    .unwrap();

    assert_eq!(
        retired_sandbox_values(&db).await.unwrap(),
        vec![
            ("image_source", "dockerfile".to_string()),
            ("dockerfile_path", "/repo/docker/Dockerfile".to_string()),
        ]
    );
    // `get` still works and carries no trace of either — the row is wider than the struct,
    // which is exactly what `SELECT *` + a narrower `row_to_config` buys.
    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.session_cap, Some(19), "the real knobs still read");
    let json = serde_json::to_value(&cfg).unwrap();
    for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        assert!(
            json.get(column).is_none(),
            "`{column}` must not come back through InstanceConfig: {json}"
        );
    }
}

/// `''` was the clear sentinel, so it never meant a value — reporting it would send the user
/// hunting for a setting they had already cleared.
#[tokio::test]
async fn an_empty_retired_value_is_not_reported() {
    let db = pre_471_db().await;
    sqlx::query("UPDATE instance_config SET image_source = '' WHERE id = 1")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(retired_sandbox_values(&db).await.unwrap(), Vec::new());
}

#[tokio::test]
async fn triggers_paused_defaults_off() {
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

/// A database as a pre-#471 daemon left it: the two retired columns present, one real knob set.
/// Built by hand — `init` cannot produce this shape any more, which is the point.
async fn pre_471_db() -> SqlitePool {
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
            default_model      TEXT,
            triggers_paused    INTEGER,
            image_source       TEXT,
            default_sandbox    TEXT,
            dockerfile_path    TEXT,
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
    init(&db).await.unwrap(); // a real boot runs it; it must not drop the columns either
    db
}
