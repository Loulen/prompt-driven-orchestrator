//! Persistence for Projets (ADR-0046): the `projects` and `project_members`
//! tables. A row exists only once a human names a group.

use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, ProjectRepository, StorageError, StoreResult};
use crate::domain::project::{project_id, Project, StoredSetting};

/// A new Projet id, stamped with the current UTC time.
pub fn generate_project_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    project_id(&ts, &uuid::Uuid::new_v4().to_string())
}

fn stored(text: Option<String>) -> Option<StoredSetting> {
    text.filter(|s| !s.trim().is_empty()).map(StoredSetting)
}

fn row_to_project(row: &sqlx::sqlite::SqliteRow, members: Vec<String>) -> Project {
    Project {
        id: row.get("id"),
        name: row.get("name"),
        harness: row
            .get::<Option<String>, _>("harness")
            .filter(|s| !s.is_empty()),
        auto_fail: row.get::<Option<i64>, _>("auto_fail").map(|v| v != 0),
        agent_choice: stored(row.get::<Option<String>, _>("agent_choice")),
        skills: stored(row.try_get::<Option<String>, _>("skills").unwrap_or(None)),
        members,
        attributes: Default::default(),
    }
}

async fn members_of(db: &SqlitePool, project_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT path FROM project_members WHERE project_id = ? ORDER BY rowid ASC")
            .bind(project_id)
            .fetch_all(db)
            .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("path")).collect())
}

/// Materialisation on naming (ADR-0046): the only path that creates a
/// `projects` row from a bare name.
pub async fn create(db: &SqlitePool, name: &str) -> Result<Project, sqlx::Error> {
    let id = generate_project_id();
    let now = pdo_kernel::clock::now_iso();
    sqlx::query("INSERT INTO projects (id, name, harness, created_at) VALUES (?, ?, NULL, ?)")
        .bind(&id)
        .bind(name)
        .bind(&now)
        .execute(db)
        .await?;
    Ok(Project::named(id, name))
}

/// Every Projet, newest first.
pub async fn list(db: &SqlitePool) -> Result<Vec<Project>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM projects ORDER BY created_at DESC")
        .fetch_all(db)
        .await?;
    let mut projects = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.get("id");
        let members = members_of(db, &id).await?;
        projects.push(row_to_project(row, members));
    }
    Ok(projects)
}

pub async fn get(db: &SqlitePool, id: &str) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    match row {
        Some(row) => {
            let members = members_of(db, id).await?;
            Ok(Some(row_to_project(&row, members)))
        }
        None => Ok(None),
    }
}

/// The Projet that owns `path`, compared **verbatim** (ADR-0033).
pub async fn owner_of(db: &SqlitePool, path: &str) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query("SELECT project_id FROM project_members WHERE path = ?")
        .bind(path)
        .fetch_optional(db)
        .await?;
    match row {
        Some(row) => get(db, &row.get::<String, _>("project_id")).await,
        None => Ok(None),
    }
}

/// The `project` tier of the harness resolver. An empty stored harness is
/// already `None`.
pub async fn harness_for_path(db: &SqlitePool, path: &str) -> Result<Option<String>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.harness))
}

/// The `project` tier of the auto-fail preference. `None` ⇒ no Projet owns the
/// path, or it states no preference — transparent either way.
pub async fn auto_fail_for_path(db: &SqlitePool, path: &str) -> Result<Option<bool>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.auto_fail))
}

/// Set one column of a Projet. `sql` is one of the `UPDATE` literals below.
async fn set_column<'q, T>(
    db: &SqlitePool,
    sql: &'static str,
    id: &'q str,
    value: T,
) -> Result<(), sqlx::Error>
where
    T: 'q + sqlx::Encode<'q, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send,
{
    sqlx::query(sql).bind(value).bind(id).execute(db).await?;
    Ok(())
}

const SET_NAME: &str = "UPDATE projects SET name = ? WHERE id = ?";
const SET_HARNESS: &str = "UPDATE projects SET harness = ? WHERE id = ?";
const SET_AUTO_FAIL: &str = "UPDATE projects SET auto_fail = ? WHERE id = ?";
const SET_AGENT_CHOICE: &str = "UPDATE projects SET agent_choice = ? WHERE id = ?";
const SET_SKILLS: &str = "UPDATE projects SET skills = ? WHERE id = ?";

async fn add_member(db: &SqlitePool, project_id: &str, path: &str) -> Result<(), sqlx::Error> {
    let now = pdo_kernel::clock::now_iso();
    sqlx::query("INSERT INTO project_members (path, project_id, created_at) VALUES (?, ?, ?)")
        .bind(path)
        .bind(project_id)
        .bind(&now)
        .execute(db)
        .await?;
    Ok(())
}

async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM project_members WHERE project_id = ?")
        .bind(id)
        .execute(db)
        .await?;
    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

fn store<T>(result: Result<T, sqlx::Error>) -> StoreResult<T> {
    result.map_err(|e| StorageError(e.to_string()))
}

/// The [`ProjectRepository`] over the daemon's SQLite database.
#[derive(Clone)]
pub struct SqliteProjects {
    db: SqlitePool,
}

impl SqliteProjects {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl ProjectRepository for SqliteProjects {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Project>>> {
        Box::pin(async move { store(list(&self.db).await) })
    }
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Project>>> {
        Box::pin(async move { store(get(&self.db, id).await) })
    }
    fn owner_of<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Option<Project>>> {
        Box::pin(async move { store(owner_of(&self.db, path).await) })
    }
    fn create<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Project>> {
        Box::pin(async move { store(create(&self.db, name).await) })
    }
    fn rename<'a>(&'a self, id: &'a str, name: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move { store(set_column(&self.db, SET_NAME, id, name).await) })
    }
    fn set_harness<'a>(
        &'a self,
        id: &'a str,
        harness: Option<&'a str>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move { store(set_column(&self.db, SET_HARNESS, id, harness).await) })
    }
    fn set_auto_fail<'a>(
        &'a self,
        id: &'a str,
        auto_fail: Option<bool>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        // `NULL` is "states none", distinct from a stored `false`.
        let stored: Option<i64> = auto_fail.map(i64::from);
        Box::pin(async move { store(set_column(&self.db, SET_AUTO_FAIL, id, stored).await) })
    }
    fn set_agent_choice<'a>(
        &'a self,
        id: &'a str,
        choice: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let text = choice.map(|c| c.0);
        Box::pin(async move { store(set_column(&self.db, SET_AGENT_CHOICE, id, text).await) })
    }
    fn set_skills<'a>(
        &'a self,
        id: &'a str,
        skills: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let text = skills.map(|s| s.0);
        Box::pin(async move { store(set_column(&self.db, SET_SKILLS, id, text).await) })
    }
    fn add_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move { store(add_member(&self.db, id, path).await) })
    }
    fn remove_member<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            store(
                sqlx::query("DELETE FROM project_members WHERE path = ?")
                    .bind(path)
                    .execute(&self.db)
                    .await
                    .map(|_| ()),
            )
        })
    }
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move { store(delete(&self.db, id).await) })
    }
}
