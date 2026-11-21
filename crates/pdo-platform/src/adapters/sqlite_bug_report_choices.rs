//! The last Destination de bug report of each Actor in SQLite
//! (`bug_report_choices` of `pdo.db`): one row per Actor, keyed by the
//! Actor's JSON, as its Apparence is.

use pdo_kernel::Actor;
use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, BugReportChoices, StorageError};

pub struct SqliteBugReportChoices {
    db: SqlitePool,
}

impl SqliteBugReportChoices {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

/// The key of `actor`'s row: its JSON, whose field order serde fixes.
fn key(actor: &Actor) -> String {
    serde_json::to_string(actor).unwrap_or_else(|_| actor.kind().to_string())
}

fn storage(error: sqlx::Error) -> StorageError {
    StorageError(error.to_string())
}

impl BugReportChoices for SqliteBugReportChoices {
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async move {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS bug_report_choices (
                    actor       TEXT PRIMARY KEY,
                    destination TEXT NOT NULL,
                    updated_at  TEXT NOT NULL
                )",
            )
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn last<'a>(&'a self, actor: &'a Actor) -> BoxFuture<'a, Result<Option<String>, StorageError>> {
        Box::pin(async move {
            let row = sqlx::query("SELECT destination FROM bug_report_choices WHERE actor = ?")
                .bind(key(actor))
                .fetch_optional(&self.db)
                .await
                .map_err(storage)?;
            Ok(row.map(|row| row.get::<String, _>("destination")))
        })
    }

    fn remember<'a>(
        &'a self,
        actor: &'a Actor,
        destination: &'a str,
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO bug_report_choices (actor, destination, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT(actor) DO UPDATE SET destination = excluded.destination, \
                 updated_at = excluded.updated_at",
            )
            .bind(key(actor))
            .bind(destination)
            .bind(pdo_kernel::clock::now_iso())
            .execute(&self.db)
            .await
            .map_err(storage)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn choices() -> SqliteBugReportChoices {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let choices = SqliteBugReportChoices::new(db);
        choices.init().await.unwrap();
        choices.init().await.unwrap();
        choices
    }

    #[tokio::test]
    async fn each_actor_keeps_its_own_last_choice() {
        let choices = choices().await;
        let alice = Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        };
        assert_eq!(choices.last(&alice).await.unwrap(), None);
        choices.remember(&alice, "desk/inbox").await.unwrap();
        choices
            .remember(&Actor::LocalOperator, "github")
            .await
            .unwrap();
        assert_eq!(
            choices.last(&alice).await.unwrap().as_deref(),
            Some("desk/inbox")
        );
        choices.remember(&alice, "github").await.unwrap();
        assert_eq!(
            choices.last(&alice).await.unwrap().as_deref(),
            Some("github")
        );
        assert_eq!(
            choices
                .last(&Actor::LocalOperator)
                .await
                .unwrap()
                .as_deref(),
            Some("github")
        );
    }
}
