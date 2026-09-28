use pdo_trigger::adapters::sqlite::*;
use pdo_trigger::domain::trigger::FireRecord;

use super::{sample, test_db};

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
            source: None,
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
            source: None,
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

// --- #341: fire origin (`source` column) ---

#[tokio::test]
async fn source_round_trips_and_rollup_still_advances_on_a_manual_fire() {
    let db = test_db().await;
    let t = create(&db, sample("sourced", "* * * * *")).await.unwrap();

    record_fire(
        &db,
        &t.id,
        &FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            run_id: Some("20260713-080000-abc1234".to_string()),
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some("manual".to_string()),
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
            run_id: Some("20260713-090000-def5678".to_string()),
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: Some("cron".to_string()),
        },
    )
    .await
    .unwrap();
    // A record with no source stores NULL (≈ cron for legacy readers).
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
            source: None,
        },
    )
    .await
    .unwrap();

    let history = fire_history(&db, &t.id).await.unwrap();
    assert_eq!(history.len(), 3);
    // Newest first.
    assert!(history[0].source.is_none());
    assert_eq!(history[1].source.as_deref(), Some("cron"));
    assert_eq!(history[2].source.as_deref(), Some("manual"));

    // The rollup advanced on the manual fire (a manual fire IS a fire).
    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(after.last_outcome.as_deref(), Some("skipped-overlap"));
    assert!(after.last_fired_at.is_some());
}

/// #341 migration: a `trigger_fires` table predating the `source` column
/// must pick it up on the next `init`, idempotently — mirror of the #244
/// guard-output migration test below.
#[tokio::test]
async fn init_adds_source_column_to_legacy_trigger_fires_table() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    // Pre-#341 schema: `trigger_fires` WITHOUT `source` (but with #244 cols).
    sqlx::query(
        "CREATE TABLE trigger_fires (
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
    .execute(&db)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO trigger_fires (trigger_id, ts, outcome, reason, run_id)
         VALUES ('trg-legacy', '2026-01-01T00:00:00.000Z', 'fired', NULL, 'run-1')",
    )
    .execute(&db)
    .await
    .unwrap();

    let before =
        sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = 'source'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(before.is_none(), "precondition: legacy table lacks source");

    init(&db).await.unwrap();

    let after =
        sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = 'source'")
            .fetch_optional(&db)
            .await
            .unwrap();
    assert!(after.is_some(), "init must add the source column");

    // The legacy row reads back with a NULL source (≈ cron).
    let history = fire_history(&db, "trg-legacy").await.unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].source.is_none());

    // A second init is a no-op.
    init(&db).await.unwrap();
    assert_eq!(fire_history(&db, "trg-legacy").await.unwrap().len(), 1);
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
        let before = sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = ?")
            .bind(col)
            .fetch_optional(&db)
            .await
            .unwrap();
        assert!(before.is_none(), "precondition: legacy table lacks {col}");
    }

    // init migrates them additively.
    init(&db).await.unwrap();

    for col in ["guard_stdout", "guard_stderr", "guard_exit_code"] {
        let after = sqlx::query("SELECT 1 FROM pragma_table_info('trigger_fires') WHERE name = ?")
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
