//! The validations of the pipelines over SQLite: one `pipeline_validation`
//! row per pipeline that states one (absent ⇒ every Projet), and the Projets
//! of a `projects` validation — or those an `all` validation is listed first
//! for — in `pipeline_validation_projects`, in the order given. The same
//! tables the daemon has always written, so an instance keeps its validations
//! across the update.

use std::collections::HashMap;

use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, PipelineValidations};
use crate::domain::validation::PipelineValidation;

const ALL: &str = "all";
const NONE: &str = "none";
const PROJECTS: &str = "projects";

/// Create the tables. Idempotent.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_validation (
            pipeline_id TEXT PRIMARY KEY,
            kind        TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_validation_projects (
            pipeline_id TEXT NOT NULL,
            project_id  TEXT NOT NULL,
            PRIMARY KEY (pipeline_id, project_id)
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

pub struct SqlitePipelineValidations {
    db: SqlitePool,
}

impl SqlitePipelineValidations {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    async fn read_all(&self) -> Result<HashMap<String, PipelineValidation>, sqlx::Error> {
        let mut projects: HashMap<String, Vec<String>> = HashMap::new();
        for row in sqlx::query(
            "SELECT pipeline_id, project_id FROM pipeline_validation_projects ORDER BY rowid ASC",
        )
        .fetch_all(&self.db)
        .await?
        {
            projects
                .entry(row.get("pipeline_id"))
                .or_default()
                .push(row.get("project_id"));
        }
        let mut all = HashMap::new();
        for row in sqlx::query("SELECT pipeline_id, kind FROM pipeline_validation")
            .fetch_all(&self.db)
            .await?
        {
            let id: String = row.get("pipeline_id");
            let kind: String = row.get("kind");
            let ids = projects.remove(&id).unwrap_or_default();
            all.insert(id, from_row(&kind, ids));
        }
        Ok(all)
    }

    async fn write(&self, id: &str, validation: &PipelineValidation) -> Result<(), sqlx::Error> {
        let kind = match validation {
            PipelineValidation::All { .. } => ALL,
            PipelineValidation::None => NONE,
            PipelineValidation::Projects { .. } => PROJECTS,
        };
        let mut tx = self.db.begin().await?;
        sqlx::query("DELETE FROM pipeline_validation_projects WHERE pipeline_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO pipeline_validation (pipeline_id, kind, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(pipeline_id) DO UPDATE SET kind = excluded.kind, updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(kind)
        .bind(pdo_kernel::clock::now_iso())
        .execute(&mut *tx)
        .await?;
        for project_id in validation.project_ids() {
            sqlx::query(
                "INSERT OR IGNORE INTO pipeline_validation_projects (pipeline_id, project_id) VALUES (?, ?)",
            )
            .bind(id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    async fn delete(&self, id: &str) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;
        sqlx::query("DELETE FROM pipeline_validation_projects WHERE pipeline_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM pipeline_validation WHERE pipeline_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;
        // Whatever sat under the new id belongs to a pipeline that is gone.
        sqlx::query("DELETE FROM pipeline_validation_projects WHERE pipeline_id = ?")
            .bind(to)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM pipeline_validation WHERE pipeline_id = ?")
            .bind(to)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE pipeline_validation SET pipeline_id = ? WHERE pipeline_id = ?")
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE pipeline_validation_projects SET pipeline_id = ? WHERE pipeline_id = ?",
        )
        .bind(to)
        .bind(from)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
}

/// A row as the domain reads it. A `projects` row whose Projets are all gone
/// is a test pipeline; an `all` row reads its Projets as the ones it is listed
/// first for; a kind this build does not know reads as « every Projet », like
/// an absent row.
fn from_row(kind: &str, project_ids: Vec<String>) -> PipelineValidation {
    match kind {
        NONE => PipelineValidation::None,
        PROJECTS => PipelineValidation::Projects { project_ids }.normalized(),
        ALL => PipelineValidation::All { project_ids }.normalized(),
        _ => PipelineValidation::all(),
    }
}

impl PipelineValidations for SqlitePipelineValidations {
    fn all(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineValidation>, String>> {
        Box::pin(async move { self.read_all().await.map_err(|e| e.to_string()) })
    }

    fn put<'a>(
        &'a self,
        id: &'a str,
        validation: &'a PipelineValidation,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.write(id, validation).await.map_err(|e| e.to_string()) })
    }

    fn forget<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.delete(id).await.map_err(|e| e.to_string()) })
    }

    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.rename(from, to).await.map_err(|e| e.to_string()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> SqlitePipelineValidations {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        SqlitePipelineValidations::new(db)
    }

    fn projects(ids: &[&str]) -> PipelineValidation {
        PipelineValidation::Projects {
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn stores_replaces_and_keeps_the_order_of_the_projets() {
        let store = store().await;
        assert!(store.all().await.unwrap().is_empty());
        store.put("p", &projects(&["b", "a"])).await.unwrap();
        store.put("t", &PipelineValidation::None).await.unwrap();
        let all = store.all().await.unwrap();
        assert_eq!(all["p"], projects(&["b", "a"]));
        assert_eq!(all["t"], PipelineValidation::None);

        store.put("p", &PipelineValidation::all()).await.unwrap();
        assert_eq!(store.all().await.unwrap()["p"], PipelineValidation::all());
        store.put("p", &projects(&["c"])).await.unwrap();
        assert_eq!(store.all().await.unwrap()["p"], projects(&["c"]));
    }

    #[tokio::test]
    async fn every_projet_keeps_the_projets_it_is_listed_first_for() {
        let store = store().await;
        let listed = PipelineValidation::All {
            project_ids: vec!["b".into(), "a".into()],
        };
        store.put("p", &listed).await.unwrap();
        assert_eq!(store.all().await.unwrap()["p"], listed);

        store.put("p", &PipelineValidation::all()).await.unwrap();
        assert_eq!(store.all().await.unwrap()["p"], PipelineValidation::all());
    }

    #[tokio::test]
    async fn a_rename_carries_and_a_forget_drops() {
        let store = store().await;
        store.put("old", &projects(&["a"])).await.unwrap();
        store.put("new", &PipelineValidation::None).await.unwrap();
        store.carry("old", "new").await.unwrap();
        let all = store.all().await.unwrap();
        assert_eq!(all.get("old"), None);
        assert_eq!(all["new"], projects(&["a"]));

        store.carry("ghost", "elsewhere").await.unwrap();
        assert_eq!(store.all().await.unwrap().get("elsewhere"), None);

        store.forget("new").await.unwrap();
        assert!(store.all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_kind_reads_as_every_projet() {
        let store = store().await;
        sqlx::query(
            "INSERT INTO pipeline_validation (pipeline_id, kind, updated_at) VALUES ('p', 'later', 'x')",
        )
        .execute(&store.db)
        .await
        .unwrap();
        assert_eq!(store.all().await.unwrap()["p"], PipelineValidation::all());
    }
}
