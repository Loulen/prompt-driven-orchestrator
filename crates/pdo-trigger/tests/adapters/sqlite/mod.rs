//! The Triggers over SQLite (`adapters::sqlite`), against an in-memory database:
//! the round trips, the partial edits, the due query, the fire history and the
//! additive migrations of a legacy table. Moved from the legacy crate's
//! `trigger_store` unit tests.

mod columns;
mod crud;
mod editor;
mod fires;
mod migrations;

use pdo_trigger::adapters::sqlite::init;
use pdo_trigger::domain::trigger::NewTrigger;
use sqlx::SqlitePool;

pub(crate) async fn test_db() -> SqlitePool {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init(&db).await.unwrap();
    db
}

pub(crate) fn sample(name: &str, cron: &str) -> NewTrigger {
    NewTrigger {
        name: name.to_string(),
        pipeline_id: "lib-pipe-1".to_string(),
        pipeline_name: "Auditor".to_string(),
        target_repo: Some("/repos/foo".to_string()),
        project_id: None,
        target_repos: None,
        source_branch: Some("main".to_string()),
        input_template: "audit the codebase".to_string(),
        variables: serde_json::json!({"depth": "full"}),
        cron: cron.to_string(),
        guard_command: None,
        overlap_policy: "skip".to_string(),
        max_concurrent: None,
        sandbox: None,
        harness: None,
        agent_choice: None,
        skills: Vec::new(),
        auto_name: true,
        next_fire_at: Some("2026-06-06T10:00:00.000Z".to_string()),
        last_editor: None,
    }
}
