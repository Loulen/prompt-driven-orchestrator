//! The skills index: one row per skill, the label unique case-insensitively.

use std::collections::BTreeMap;

use sqlx::{Row, SqlitePool};

use super::storage;
use crate::application::ports::{BoxFuture, SkillIndex, StoreResult};
use crate::domain::seed::is_seeded;
use crate::domain::skill::{Provenance, ProvidedBy, Skill};

/// The [`SkillIndex`] port over SQLite.
pub struct SqliteSkillIndex {
    db: SqlitePool,
}

impl SqliteSkillIndex {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn row_to_skill(row: &sqlx::sqlite::SqliteRow) -> Skill {
    let url: Option<String> = row.get("source");
    let extension = row
        .get::<Option<String>, _>("extension")
        .map(|id| ProvidedBy {
            id,
            version: row
                .get::<Option<String>, _>("extension_version")
                .unwrap_or_default(),
        });
    Skill {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        folder_id: row.get("folder_id"),
        locked: is_seeded(row.get("id")) || extension.is_some(),
        source: url.map(|url| Provenance {
            url,
            git_ref: row.get("source_ref"),
            commit: row.get("source_commit"),
            path: row
                .get::<Option<String>, _>("source_path")
                .unwrap_or_default(),
        }),
        extension,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// The `id → name` map of the whole bank, for a legacy caller that still reads
/// the database directly.
pub async fn snapshot_names(db: &SqlitePool) -> Result<BTreeMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name FROM skills")
        .fetch_all(db)
        .await?;
    Ok(rows
        .iter()
        .map(|row| (row.get::<String, _>("id"), row.get::<String, _>("name")))
        .collect())
}

impl SkillIndex for SqliteSkillIndex {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Skill>>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT * FROM skills ORDER BY name COLLATE NOCASE ASC, created_at ASC",
            )
            .fetch_all(&self.db)
            .await
            .map_err(storage)?;
            Ok(rows.iter().map(row_to_skill).collect())
        })
    }

    fn names(&self) -> BoxFuture<'_, StoreResult<BTreeMap<String, String>>> {
        Box::pin(async move { snapshot_names(&self.db).await.map_err(storage) })
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>> {
        Box::pin(async move {
            let row = sqlx::query("SELECT * FROM skills WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.db)
                .await
                .map_err(storage)?;
            Ok(row.as_ref().map(row_to_skill))
        })
    }

    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT * FROM skills WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.db)
            .await
            .map_err(storage)?;
            Ok(row.as_ref().map(row_to_skill))
        })
    }

    fn insert<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let source = skill.source.as_ref();
            sqlx::query(
                "INSERT INTO skills (id, name, description, folder_id, source, source_ref, \
                 source_commit, source_path, extension, extension_version, created_at, \
                 updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&skill.id)
            .bind(&skill.name)
            .bind(&skill.description)
            .bind(&skill.folder_id)
            .bind(source.map(|p| &p.url))
            .bind(source.and_then(|p| p.git_ref.as_ref()))
            .bind(source.and_then(|p| p.commit.as_ref()))
            .bind(source.map(|p| &p.path))
            .bind(skill.extension.as_ref().map(|e| &e.id))
            .bind(skill.extension.as_ref().map(|e| &e.version))
            .bind(&skill.created_at)
            .bind(&skill.updated_at)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let source = skill.source.as_ref();
            sqlx::query(
                "UPDATE skills SET name = ?, description = ?, folder_id = ?, source = ?, \
                 source_ref = ?, source_commit = ?, source_path = ?, extension = ?, \
                 extension_version = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&skill.name)
            .bind(&skill.description)
            .bind(&skill.folder_id)
            .bind(source.map(|p| &p.url))
            .bind(source.and_then(|p| p.git_ref.as_ref()))
            .bind(source.and_then(|p| p.commit.as_ref()))
            .bind(source.map(|p| &p.path))
            .bind(skill.extension.as_ref().map(|e| &e.id))
            .bind(skill.extension.as_ref().map(|e| &e.version))
            .bind(&skill.updated_at)
            .bind(&skill.id)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a Provenance,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE skills SET source = ?, source_ref = ?, source_commit = ?, source_path = ? \
                 WHERE id = ?",
            )
            .bind(&provenance.url)
            .bind(&provenance.git_ref)
            .bind(&provenance.commit)
            .bind(&provenance.path)
            .bind(id)
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let res = sqlx::query("DELETE FROM skills WHERE id = ?")
                .bind(id)
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(res.rows_affected() > 0)
        })
    }

    fn refile<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            sqlx::query("UPDATE skills SET folder_id = ?, updated_at = ? WHERE folder_id = ?")
                .bind(to)
                .bind(now)
                .bind(from)
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(())
        })
    }
}
