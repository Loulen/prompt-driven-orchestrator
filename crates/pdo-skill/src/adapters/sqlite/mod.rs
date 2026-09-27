//! The SQLite side of the bank: the skills index, the folders, and the
//! sources the operator scanned.

mod folders;
mod skills;
mod sources;

pub use folders::SqliteFolderIndex;
pub use skills::{snapshot_names, SqliteSkillIndex};
pub use sources::SqliteSourceHistory;

use sqlx::{Row, SqlitePool};

use crate::domain::error::SkillError;

pub(crate) fn storage(error: sqlx::Error) -> SkillError {
    SkillError::Storage(error.to_string())
}

/// Create the index tables if absent, and grow them to the current shape.
/// Idempotent. The seed itself runs at daemon startup, not here.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skill_folders (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            parent_id  TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skills (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            description   TEXT NOT NULL,
            folder_id     TEXT,
            source        TEXT,
            source_commit TEXT,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_name_nocase \
         ON skills(name COLLATE NOCASE)",
    )
    .execute(db)
    .await?;
    // #670: provenance grows from `(source, source_commit)` to a full object on
    // the skill, and lands on the folder too. Additive columns, guarded so a
    // 1.51 database opens unchanged.
    for (table, column, ty) in [
        ("skills", "source_ref", "TEXT"),
        ("skills", "source_path", "TEXT"),
        ("skill_folders", "source", "TEXT"),
        ("skill_folders", "source_ref", "TEXT"),
        ("skill_folders", "source_commit", "TEXT"),
        ("skill_folders", "source_path", "TEXT"),
        ("skill_folders", "source_imported_at", "TEXT"),
        ("skill_folders", "source_found", "INTEGER"),
        ("skill_folders", "source_invalid", "INTEGER"),
    ] {
        add_column_if_missing(db, table, column, ty).await?;
    }
    // Sources the operator has scanned, for the import popup's "Recent sources".
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS skill_sources (
            url          TEXT NOT NULL,
            git_ref      TEXT NOT NULL DEFAULT '',
            path         TEXT NOT NULL DEFAULT '',
            last_used_at TEXT NOT NULL,
            PRIMARY KEY (url, git_ref, path)
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

async fn add_column_if_missing(
    db: &SqlitePool,
    table: &str,
    column: &str,
    ty: &str,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(db)
        .await?;
    let present = rows
        .iter()
        .any(|row| row.get::<String, _>("name") == column);
    if !present {
        sqlx::query(&format!("ALTER TABLE {table} ADD COLUMN {column} {ty}"))
            .execute(db)
            .await?;
    }
    Ok(())
}
