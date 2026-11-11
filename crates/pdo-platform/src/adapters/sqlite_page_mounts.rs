//! Page mounts in SQLite: the `page_mounts` table of `pdo.db`. The guide and
//! the owner are JSON columns of the mount's row, so they go with the row.

use pdo_kernel::Actor;
use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, InsertError, PageMountRepository, StorageError};
use crate::domain::page_mount::PageMount;

pub struct SqlitePageMounts {
    db: SqlitePool,
}

impl SqlitePageMounts {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn storage(error: sqlx::Error) -> StorageError {
    StorageError(error.to_string())
}

const COLUMNS: &str = "name, directory, run_id, node_id, created_at, guide, owner, project_id";

/// A JSON column, decoded.
fn json_column<T: serde::de::DeserializeOwned>(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<T>, sqlx::Error> {
    let raw: Option<String> = row.try_get(column)?;
    raw.map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn row_to_mount(row: &sqlx::sqlite::SqliteRow) -> Result<PageMount, sqlx::Error> {
    Ok(PageMount {
        name: row.try_get("name")?,
        directory: row.try_get("directory")?,
        run_id: row.try_get("run_id")?,
        node_id: row.try_get("node_id")?,
        created_at: row.try_get("created_at")?,
        guide: json_column(row, "guide")?,
        owner: json_column(row, "owner")?,
        project_id: row.try_get("project_id")?,
    })
}

/// A value as its JSON column holds it.
fn to_json<T: serde::Serialize>(value: Option<&T>) -> Result<Option<String>, StorageError> {
    value
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| StorageError(e.to_string()))
}

async fn get(db: &SqlitePool, name: &str) -> Result<Option<PageMount>, sqlx::Error> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM page_mounts WHERE name = ?"))
        .bind(name)
        .fetch_optional(db)
        .await?;
    row.as_ref().map(row_to_mount).transpose()
}

/// Add `column` to `page_mounts` when a database older than it lacks it.
async fn add_column(db: &SqlitePool, column: &str) -> Result<(), sqlx::Error> {
    let present: Option<(String,)> =
        sqlx::query_as("SELECT name FROM pragma_table_info('page_mounts') WHERE name = ?")
            .bind(column)
            .fetch_optional(db)
            .await?;
    if present.is_none() {
        sqlx::query(&format!("ALTER TABLE page_mounts ADD COLUMN {column} TEXT"))
            .execute(db)
            .await?;
    }
    Ok(())
}

impl PageMountRepository for SqlitePageMounts {
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async move {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS page_mounts (
                    name       TEXT PRIMARY KEY,
                    directory  TEXT NOT NULL,
                    run_id     TEXT,
                    created_at TEXT NOT NULL
                )",
            )
            .execute(&self.db)
            .await
            .map_err(storage)?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_page_mounts_run_id ON page_mounts(run_id)")
                .execute(&self.db)
                .await
                .map_err(storage)?;
            for column in ["node_id", "guide", "owner", "project_id"] {
                add_column(&self.db, column).await.map_err(storage)?;
            }
            Ok(())
        })
    }

    fn insert<'a>(&'a self, mount: &'a PageMount) -> BoxFuture<'a, Result<(), InsertError>> {
        Box::pin(async move {
            let guide = to_json(mount.guide.as_ref())?;
            let owner = to_json(mount.owner.as_ref())?;
            let result = sqlx::query(&format!(
                "INSERT INTO page_mounts ({COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            ))
            .bind(&mount.name)
            .bind(&mount.directory)
            .bind(&mount.run_id)
            .bind(&mount.node_id)
            .bind(&mount.created_at)
            .bind(guide)
            .bind(owner)
            .bind(&mount.project_id)
            .execute(&self.db)
            .await;
            match result {
                Ok(_) => Ok(()),
                Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                    Err(InsertError::Duplicate)
                }
                Err(error) => Err(InsertError::Storage(storage(error))),
            }
        })
    }

    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>> {
        Box::pin(async move { get(&self.db, name).await.map_err(storage) })
    }

    fn list(&self) -> BoxFuture<'_, Result<Vec<PageMount>, StorageError>> {
        Box::pin(async move {
            let rows = sqlx::query(&format!(
                "SELECT {COLUMNS} FROM page_mounts ORDER BY name ASC"
            ))
            .fetch_all(&self.db)
            .await
            .map_err(storage)?;
            rows.iter()
                .map(row_to_mount)
                .collect::<Result<_, _>>()
                .map_err(storage)
        })
    }

    fn remove<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>> {
        Box::pin(async move {
            let Some(mount) = get(&self.db, name).await.map_err(storage)? else {
                return Ok(None);
            };
            sqlx::query("DELETE FROM page_mounts WHERE name = ?")
                .bind(name)
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(Some(mount))
        })
    }

    fn place<'a>(
        &'a self,
        name: &'a str,
        owner: Option<&'a Actor>,
        project_id: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            sqlx::query("UPDATE page_mounts SET owner = ?, project_id = ? WHERE name = ?")
                .bind(to_json(owner)?)
                .bind(project_id)
                .bind(name)
                .execute(&self.db)
                .await
                .map_err(storage)?;
            Ok(())
        })
    }

    fn remove_for_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<u64, StorageError>> {
        Box::pin(async move {
            Ok(sqlx::query("DELETE FROM page_mounts WHERE run_id = ?")
                .bind(run_id)
                .execute(&self.db)
                .await
                .map_err(storage)?
                .rows_affected())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::page_guide::GuideEntry;

    async fn repository() -> SqlitePageMounts {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let repo = SqlitePageMounts::new(db);
        repo.init().await.unwrap();
        repo.init().await.expect("init is idempotent");
        repo
    }

    fn mount(name: &str, run_id: Option<&str>) -> PageMount {
        PageMount {
            name: name.into(),
            directory: format!("/srv/{name}"),
            run_id: run_id.map(str::to_string),
            node_id: run_id.map(|_| "n1".to_string()),
            created_at: "2026-09-26T00:00:00.000Z".into(),
            guide: None,
            owner: run_id.map(|_| Actor::Identified {
                namespace: "test".into(),
                id: "alice".into(),
            }),
            project_id: run_id.map(|_| "p1".to_string()),
        }
    }

    #[tokio::test]
    async fn mounts_round_trip_by_name() {
        let repo = repository().await;
        repo.insert(&mount("b", None)).await.unwrap();
        repo.insert(&mount("a", Some("r1"))).await.unwrap();
        assert_eq!(repo.get("a").await.unwrap(), Some(mount("a", Some("r1"))));
        assert_eq!(repo.get("zz").await.unwrap(), None);
        let names: Vec<String> = repo
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, ["a", "b"], "listed by name");
    }

    #[tokio::test]
    async fn the_guide_is_kept_with_its_mount_and_goes_with_it() {
        let repo = repository().await;
        let mut guided = mount("g", Some("r1"));
        guided.guide = Some(vec![GuideEntry {
            title: "Start here".into(),
            text: "The chosen option.".into(),
            link: "#a".into(),
        }]);
        repo.insert(&guided).await.unwrap();
        assert_eq!(repo.get("g").await.unwrap(), Some(guided.clone()));
        assert_eq!(repo.list().await.unwrap(), vec![guided]);
        assert_eq!(repo.remove_for_run("r1").await.unwrap(), 1);
        assert_eq!(repo.get("g").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_table_older_than_the_guide_gains_its_columns() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE page_mounts (name TEXT PRIMARY KEY, directory TEXT NOT NULL, \
             run_id TEXT, created_at TEXT NOT NULL)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO page_mounts VALUES ('old', '/srv/old', NULL, 'then')")
            .execute(&db)
            .await
            .unwrap();
        let repo = SqlitePageMounts::new(db);
        repo.init().await.unwrap();
        let old = repo.get("old").await.unwrap().unwrap();
        assert_eq!((old.node_id, old.guide), (None, None));
        assert_eq!((old.owner, old.project_id), (None, None));
    }

    #[tokio::test]
    async fn a_mount_is_placed_with_its_owner_and_its_projet() {
        let repo = repository().await;
        repo.insert(&mount("a", None)).await.unwrap();
        let bob = Actor::Identified {
            namespace: "test".into(),
            id: "bob".into(),
        };
        repo.place("a", Some(&bob), Some("p2")).await.unwrap();
        let placed = repo.get("a").await.unwrap().unwrap();
        assert_eq!(placed.owner, Some(bob));
        assert_eq!(placed.project_id.as_deref(), Some("p2"));
        assert_eq!(repo.list().await.unwrap(), vec![placed]);
    }

    #[tokio::test]
    async fn a_taken_name_is_a_duplicate() {
        let repo = repository().await;
        repo.insert(&mount("a", None)).await.unwrap();
        assert_eq!(
            repo.insert(&mount("a", None)).await,
            Err(InsertError::Duplicate)
        );
    }

    #[tokio::test]
    async fn removal_returns_what_went_and_a_run_takes_its_mounts_along() {
        let repo = repository().await;
        repo.insert(&mount("a", Some("r1"))).await.unwrap();
        repo.insert(&mount("b", Some("r1"))).await.unwrap();
        repo.insert(&mount("c", None)).await.unwrap();
        assert_eq!(repo.remove("c").await.unwrap(), Some(mount("c", None)));
        assert_eq!(repo.remove("c").await.unwrap(), None);
        assert_eq!(repo.remove_for_run("r1").await.unwrap(), 2);
        assert!(repo.list().await.unwrap().is_empty());
    }
}
