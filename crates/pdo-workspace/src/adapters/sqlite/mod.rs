//! The SQLite side of the workspace: the Projets, their members, the
//! provisioning rules, and the recent repositories (read off the Runs' events).

mod projects;
mod provisioning_rules;
mod recent_repositories;

pub use projects::*;
pub use provisioning_rules::*;
pub use recent_repositories::*;

use sqlx::SqlitePool;

/// Create (or migrate) the workspace's tables. Idempotent.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    init_projects(db).await?;
    init_provisioning_rules(db).await
}

async fn init_projects(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT,
            auto_fail  INTEGER,
            agent_choice TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Pre-ADR-0049 databases have no `auto_fail` column. Guarded `ADD COLUMN`,
    // NULLABLE so an existing Projet states no preference.
    let has_auto_fail =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'auto_fail'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_auto_fail {
        sqlx::query("ALTER TABLE projects ADD COLUMN auto_fail INTEGER")
            .execute(db)
            .await?;
    }

    // Pre-#563 databases have no `agent_choice` column. Guarded `ADD COLUMN`,
    // NULLABLE so an existing Projet falls back to its legacy `harness` column.
    let has_agent_choice =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'agent_choice'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_agent_choice {
        sqlx::query("ALTER TABLE projects ADD COLUMN agent_choice TEXT")
            .execute(db)
            .await?;
    }

    // Pre-#669 databases have no `skills` column. Guarded `ADD COLUMN`, NULLABLE
    // so an existing Projet selects nothing.
    let has_skills =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'skills'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_skills {
        sqlx::query("ALTER TABLE projects ADD COLUMN skills TEXT")
            .execute(db)
            .await?;
    }

    // `path` is the PRIMARY KEY: at-most-one-Projet-per-path is a schema
    // invariant. `project_id` is indexed for the members-of lookup. No FK
    // ON DELETE cascade — deletes clear members explicitly (portable, and the
    // daemon never enables `PRAGMA foreign_keys`).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS project_members (
            path       TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_project_members_project ON project_members(project_id)",
    )
    .execute(db)
    .await?;

    Ok(())
}
