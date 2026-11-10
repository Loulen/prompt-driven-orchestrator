//! The versions of the pipelines over SQLite (ADR-0088): one
//! `pipeline_version` row per pipeline and number, with the YAML and the
//! prompts it froze.

use std::collections::{BTreeMap, HashMap};

use pdo_kernel::Actor;
use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, PipelineVersions};
use crate::domain::version::{PipelineVersion, PipelineVersionRef, VersionMeta};

/// Create the table. Idempotent.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_version (
            pipeline_id TEXT NOT NULL,
            number      INTEGER NOT NULL,
            title       TEXT NOT NULL,
            author      TEXT,
            saved_at    TEXT NOT NULL,
            external    INTEGER NOT NULL DEFAULT 0,
            yaml        TEXT NOT NULL,
            prompts     TEXT NOT NULL,
            PRIMARY KEY (pipeline_id, number)
        )",
    )
    .execute(db)
    .await
    .map(drop)
}

pub struct SqlitePipelineVersions {
    db: SqlitePool,
}

impl SqlitePipelineVersions {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    async fn read_list(&self, id: &str) -> Result<Vec<VersionMeta>, String> {
        let rows = sqlx::query(
            "SELECT number, title, author, saved_at, external FROM pipeline_version
             WHERE pipeline_id = ? ORDER BY number",
        )
        .bind(id)
        .fetch_all(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        rows.iter().map(meta).collect()
    }

    async fn read_one(&self, id: &str, number: u32) -> Result<Option<PipelineVersion>, String> {
        let row = sqlx::query(
            "SELECT number, title, author, saved_at, external, yaml, prompts FROM pipeline_version
             WHERE pipeline_id = ? AND number = ?",
        )
        .bind(id)
        .bind(i64::from(number))
        .fetch_optional(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        let Some(row) = row else { return Ok(None) };
        let prompts: String = row.get("prompts");
        let prompts: BTreeMap<String, String> =
            serde_json::from_str(&prompts).map_err(|e| e.to_string())?;
        Ok(Some(PipelineVersion {
            meta: meta(&row)?,
            yaml: row.get("yaml"),
            prompts,
        }))
    }

    /// Keep `version`; with `first`, only when `id` has no version yet.
    async fn write(&self, id: &str, version: &PipelineVersion, first: bool) -> Result<(), String> {
        let author = version
            .meta
            .author
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        let prompts = serde_json::to_string(&version.prompts).map_err(|e| e.to_string())?;
        let insert = if first {
            "INSERT INTO pipeline_version (pipeline_id, number, title, author, saved_at, external, yaml, prompts)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
             WHERE NOT EXISTS (SELECT 1 FROM pipeline_version WHERE pipeline_id = ?1)"
        } else {
            "INSERT INTO pipeline_version (pipeline_id, number, title, author, saved_at, external, yaml, prompts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(pipeline_id, number) DO UPDATE SET title = excluded.title,
               author = excluded.author, saved_at = excluded.saved_at,
               external = excluded.external, yaml = excluded.yaml, prompts = excluded.prompts"
        };
        sqlx::query(insert)
            .bind(id)
            .bind(i64::from(version.meta.number))
            .bind(&version.meta.title)
            .bind(author)
            .bind(&version.meta.saved_at)
            .bind(version.meta.external)
            .bind(&version.yaml)
            .bind(prompts)
            .execute(&self.db)
            .await
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

fn number(row: &sqlx::sqlite::SqliteRow) -> u32 {
    let number: i64 = row.get("number");
    number.max(0) as u32
}

fn meta(row: &sqlx::sqlite::SqliteRow) -> Result<VersionMeta, String> {
    let author: Option<String> = row.get("author");
    let author: Option<Actor> = author
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| e.to_string())?;
    Ok(VersionMeta {
        number: number(row),
        title: row.get("title"),
        author,
        saved_at: row.get("saved_at"),
        external: row.get("external"),
    })
}

impl PipelineVersions for SqlitePipelineVersions {
    fn list<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<VersionMeta>, String>> {
        Box::pin(self.read_list(id))
    }

    fn get<'a>(
        &'a self,
        id: &'a str,
        number: u32,
    ) -> BoxFuture<'a, Result<Option<PipelineVersion>, String>> {
        Box::pin(self.read_one(id, number))
    }

    fn put<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.write(id, version, false))
    }

    fn put_first<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.write(id, version, true))
    }

    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            sqlx::query("DELETE FROM pipeline_version WHERE pipeline_id = ?")
                .bind(id)
                .execute(&self.db)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        })
    }

    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let mut tx = self.db.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM pipeline_version WHERE pipeline_id = ?")
                .bind(to)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("UPDATE pipeline_version SET pipeline_id = ? WHERE pipeline_id = ?")
                .bind(to)
                .bind(from)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())
        })
    }

    fn lasts(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineVersionRef>, String>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT v.pipeline_id, v.number, v.title FROM pipeline_version v
                 JOIN (SELECT pipeline_id, MAX(number) AS number FROM pipeline_version GROUP BY pipeline_id) last
                   ON last.pipeline_id = v.pipeline_id AND last.number = v.number",
            )
            .fetch_all(&self.db)
            .await
            .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let reference = PipelineVersionRef {
                        number: number(&row),
                        title: row.get("title"),
                    };
                    (row.get("pipeline_id"), reference)
                })
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(number: u32, title: &str, author: Option<Actor>) -> PipelineVersion {
        PipelineVersion {
            meta: VersionMeta {
                number,
                title: title.into(),
                author,
                saved_at: "2026-10-19T10:00:00Z".into(),
                external: false,
            },
            yaml: format!("name: p\n# v{number}\n"),
            prompts: BTreeMap::from([("worker".to_string(), format!("prompt {number}"))]),
        }
    }

    async fn store() -> SqlitePipelineVersions {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        SqlitePipelineVersions::new(db)
    }

    #[tokio::test]
    async fn a_version_put_again_takes_its_new_date() {
        let store = store().await;
        store.put("p", &version(1, "v1", None)).await.unwrap();
        let mut again = version(1, "v1", Some(Actor::LocalOperator));
        again.meta.saved_at = "2026-10-19T11:30:00Z".into();
        store.put("p", &again).await.unwrap();
        let listed = store.list("p").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].saved_at, "2026-10-19T11:30:00Z");
    }

    #[tokio::test]
    async fn keeps_the_versions_of_a_pipeline_and_its_last_one() {
        let store = store().await;
        store.put("p", &version(1, "v1", None)).await.unwrap();
        store
            .put("p", &version(2, "Split", Some(Actor::LocalOperator)))
            .await
            .unwrap();
        store.put("q", &version(1, "1.0", None)).await.unwrap();
        let listed = store.list("p").await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|v| (v.number, v.title.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "v1"), (2, "Split")]
        );
        assert_eq!(listed[1].author, Some(Actor::LocalOperator));
        let two = store.get("p", 2).await.unwrap().unwrap();
        assert_eq!(two.yaml, "name: p\n# v2\n");
        assert_eq!(two.prompts["worker"], "prompt 2");
        let lasts = store.lasts().await.unwrap();
        assert_eq!(lasts["p"].number, 2);
        assert_eq!(lasts["q"].title, "1.0");
    }

    #[tokio::test]
    async fn a_put_under_a_taken_number_replaces_it() {
        let store = store().await;
        store.put("p", &version(1, "v1", None)).await.unwrap();
        store.put("p", &version(1, "Fixed", None)).await.unwrap();
        let listed = store.list("p").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Fixed");
    }

    #[tokio::test]
    async fn a_first_version_is_only_kept_where_there_is_none() {
        let store = store().await;
        store.put_first("p", &version(1, "v1", None)).await.unwrap();
        store.put("p", &version(2, "v2", None)).await.unwrap();
        store
            .put_first("p", &version(1, "late", None))
            .await
            .unwrap();
        let listed = store.list("p").await.unwrap();
        assert_eq!(
            listed.iter().map(|v| v.title.as_str()).collect::<Vec<_>>(),
            ["v1", "v2"]
        );
    }

    #[tokio::test]
    async fn the_versions_follow_a_rename_and_go_with_a_delete() {
        let store = store().await;
        store.put("old", &version(1, "v1", None)).await.unwrap();
        store.put("new", &version(1, "stale", None)).await.unwrap();
        store.carry("old", "new").await.unwrap();
        assert!(store.list("old").await.unwrap().is_empty());
        assert_eq!(store.list("new").await.unwrap()[0].title, "v1");
        store.remove("new").await.unwrap();
        assert!(store.list("new").await.unwrap().is_empty());
    }
}
