//! The folders of the bank: a free hierarchy, optionally stamped with the
//! Source it was imported from.

use sqlx::{Row, SqlitePool};

use super::storage;
use crate::application::ports::{BoxFuture, FolderIndex, StoreResult};
use crate::domain::skill::{FolderProvenance, SkillFolder};

/// The [`FolderIndex`] port over SQLite.
pub struct SqliteFolderIndex {
    db: SqlitePool,
}

impl SqliteFolderIndex {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn row_to_folder(row: &sqlx::sqlite::SqliteRow) -> SkillFolder {
    let url: Option<String> = row.get("source");
    SkillFolder {
        id: row.get("id"),
        name: row.get("name"),
        parent_id: row.get("parent_id"),
        source: url.map(|url| FolderProvenance {
            url,
            git_ref: row.get("source_ref"),
            commit: row.get("source_commit"),
            path: row
                .get::<Option<String>, _>("source_path")
                .unwrap_or_default(),
            imported_at: row
                .get::<Option<String>, _>("source_imported_at")
                .unwrap_or_default(),
            found: row.get::<Option<i64>, _>("source_found").unwrap_or(0),
            invalid: row.get::<Option<i64>, _>("source_invalid").unwrap_or(0),
        }),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

impl FolderIndex for SqliteFolderIndex {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<SkillFolder>>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT * FROM skill_folders ORDER BY name COLLATE NOCASE ASC, created_at ASC",
            )
            .fetch_all(&self.db)
            .await
            .map_err(storage)?;
            Ok(rows.iter().map(row_to_folder).collect())
        })
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<SkillFolder>>> {
        Box::pin(async move {
            let row = sqlx::query("SELECT * FROM skill_folders WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.db)
                .await
                .map_err(storage)?;
            Ok(row.as_ref().map(row_to_folder))
        })
    }

    fn root_named<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<String>>> {
        Box::pin(async move {
            sqlx::query_scalar(
                "SELECT id FROM skill_folders \
                 WHERE parent_id IS NULL AND lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.db)
            .await
            .map_err(storage)
        })
    }

    fn insert<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO skill_folders (id, name, parent_id, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&folder.id)
            .bind(&folder.name)
            .bind(&folder.parent_id)
            .bind(&folder.created_at)
            .bind(&folder.updated_at)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE skill_folders SET name = ?, parent_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&folder.name)
            .bind(&folder.parent_id)
            .bind(&folder.updated_at)
            .bind(&folder.id)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a FolderProvenance,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE skill_folders SET source = ?, source_ref = ?, source_commit = ?, \
                 source_path = ?, source_imported_at = ?, source_found = ?, source_invalid = ?, \
                 updated_at = ? WHERE id = ?",
            )
            .bind(&provenance.url)
            .bind(&provenance.git_ref)
            .bind(&provenance.commit)
            .bind(&provenance.path)
            .bind(&provenance.imported_at)
            .bind(provenance.found)
            .bind(provenance.invalid)
            .bind(now)
            .bind(id)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn reparent<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE skill_folders SET parent_id = ?, updated_at = ? WHERE parent_id = ?",
            )
            .bind(to)
            .bind(now)
            .bind(from)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query("DELETE FROM skill_folders WHERE id = ?")
                .bind(id)
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(())
        })
    }
}
