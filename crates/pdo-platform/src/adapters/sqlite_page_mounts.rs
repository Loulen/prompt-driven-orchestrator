//! Page mounts in SQLite: the `page_mounts` table of `pdo.db`.

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

fn row_to_mount(row: &sqlx::sqlite::SqliteRow) -> Result<PageMount, sqlx::Error> {
    Ok(PageMount {
        name: row.try_get("name")?,
        directory: row.try_get("directory")?,
        run_id: row.try_get("run_id")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn get(db: &SqlitePool, name: &str) -> Result<Option<PageMount>, sqlx::Error> {
    let row =
        sqlx::query("SELECT name, directory, run_id, created_at FROM page_mounts WHERE name = ?")
            .bind(name)
            .fetch_optional(db)
            .await?;
    row.as_ref().map(row_to_mount).transpose()
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
            Ok(())
        })
    }

    fn insert<'a>(&'a self, mount: &'a PageMount) -> BoxFuture<'a, Result<(), InsertError>> {
        Box::pin(async move {
            let result = sqlx::query(
                "INSERT INTO page_mounts (name, directory, run_id, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(&mount.name)
            .bind(&mount.directory)
            .bind(&mount.run_id)
            .bind(&mount.created_at)
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
            let rows = sqlx::query(
                "SELECT name, directory, run_id, created_at FROM page_mounts ORDER BY name ASC",
            )
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
            created_at: "2026-09-26T00:00:00.000Z".into(),
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
