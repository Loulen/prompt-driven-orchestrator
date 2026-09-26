//! The recent repositories, read off a real event table (the Run's `events`,
//! as the daemon creates it).

use pdo_workspace::adapters::sqlite::SqliteRecentRepositories;
use pdo_workspace::application::ports::RecentRepositories;
use sqlx::SqlitePool;

async fn events_db() -> SqlitePool {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,
            node_id TEXT,
            iter INTEGER,
            payload JSON
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    db
}

async fn seed(db: &SqlitePool, seq: u32, kind: &str, payload: serde_json::Value) {
    sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, ?, ?)")
        .bind(format!("run-{seq}"))
        .bind(format!("2026-01-01T00:00:{seq:02}.000Z"))
        .bind(kind)
        .bind(payload.to_string())
        .execute(db)
        .await
        .unwrap();
}

async fn run_started(db: &SqlitePool, seq: u32, repo: Option<&str>) {
    let mut payload = serde_json::json!({ "pipeline_name": "test-pipe", "input": "test" });
    if let Some(repo) = repo {
        payload["target_repo"] = serde_json::json!(repo);
    }
    seed(db, seq, "run_started", payload).await;
}

#[tokio::test]
async fn empty_when_no_run_started() {
    let db = events_db().await;
    let recent = SqliteRecentRepositories::new(db);
    assert!(recent.recent(5).await.unwrap().is_empty());
}

#[tokio::test]
async fn deduplicates_and_orders_by_most_recent() {
    let db = events_db().await;
    run_started(&db, 1, Some("/repo/a")).await;
    run_started(&db, 2, Some("/repo/b")).await;
    run_started(&db, 3, Some("/repo/a")).await;
    let recent = SqliteRecentRepositories::new(db);
    assert_eq!(recent.recent(5).await.unwrap(), vec!["/repo/a", "/repo/b"]);
}

#[tokio::test]
async fn stops_at_the_limit() {
    let db = events_db().await;
    for i in 0..7 {
        run_started(&db, i, Some(&format!("/repo/{i}"))).await;
    }
    let recent = SqliteRecentRepositories::new(db);
    assert_eq!(
        recent.recent(5).await.unwrap(),
        vec!["/repo/6", "/repo/5", "/repo/4", "/repo/3", "/repo/2"]
    );
}

#[tokio::test]
async fn ignores_starts_without_a_repo_and_other_events() {
    let db = events_db().await;
    run_started(&db, 1, Some("/repo/a")).await;
    run_started(&db, 2, None).await;
    seed(
        &db,
        3,
        "node_started",
        serde_json::json!({ "target_repo": "/repo/x" }),
    )
    .await;
    let recent = SqliteRecentRepositories::new(db);
    assert_eq!(recent.recent(5).await.unwrap(), vec!["/repo/a"]);
}
