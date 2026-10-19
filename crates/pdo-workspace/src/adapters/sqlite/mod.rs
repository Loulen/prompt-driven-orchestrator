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

    // The default primary repository a launch in the Projet preselects
    // (ADR-0087). Guarded `ADD COLUMN`, NULLABLE: an existing Projet
    // preselects its first member.
    let has_primary_repo =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'primary_repo'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_primary_repo {
        sqlx::query("ALTER TABLE projects ADD COLUMN primary_repo TEXT")
            .execute(db)
            .await?;
    }

    // `(project_id, path)` is the PRIMARY KEY: a path joins a Projet once,
    // and may join several (ADR-0087). `path` is indexed for the
    // Projets-of-a-path lookup. No FK ON DELETE cascade — deletes clear
    // members explicitly (portable, and the daemon never enables
    // `PRAGMA foreign_keys`).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS project_members (
            path       TEXT NOT NULL,
            project_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (project_id, path)
        )",
    )
    .execute(db)
    .await?;
    migrate_members_to_many_projects(db).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_project_members_project ON project_members(project_id)",
    )
    .execute(db)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_project_members_path ON project_members(path)")
        .execute(db)
        .await?;

    Ok(())
}

/// Before ADR-0087, `path` alone was the key of `project_members` (one Projet
/// per path). The table is rebuilt once under the `(project_id, path)` key,
/// its rows and their order kept (`rowid` is the order a path joined).
async fn migrate_members_to_many_projects(db: &SqlitePool) -> Result<(), sqlx::Error> {
    let keys: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM pragma_table_info('project_members') WHERE pk > 0 ORDER BY pk",
    )
    .fetch_all(db)
    .await?;
    if keys.len() != 1 {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    sqlx::query(
        "CREATE TABLE project_members_many (
            path       TEXT NOT NULL,
            project_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (project_id, path)
        )",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO project_members_many (path, project_id, created_at)
         SELECT path, project_id, created_at FROM project_members ORDER BY rowid ASC",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("DROP TABLE project_members")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE project_members_many RENAME TO project_members")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}
