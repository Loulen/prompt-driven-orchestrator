//! The entity attributes over SQLite (`entity_attributes`): one row per
//! entity, key and value, outside the event log (D16). The table is created,
//! idempotently, on first use.

use std::collections::BTreeMap;

use pdo_kernel::attributes::{wire_kind, AttributeStore, Attributes, BoxFuture, EntityRef};
use pdo_kernel::ResourceKind;
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS entity_attributes (
    entity_kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (entity_kind, entity_id, key)
)";

pub(crate) struct SqliteAttributes {
    db: SqlitePool,
    schema: OnceCell<()>,
}

impl SqliteAttributes {
    pub(crate) fn new(db: SqlitePool) -> Self {
        Self {
            db,
            schema: OnceCell::new(),
        }
    }

    async fn ready(&self) -> Result<&SqlitePool, String> {
        self.schema
            .get_or_try_init(|| async {
                sqlx::query(SCHEMA)
                    .execute(&self.db)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .await?;
        Ok(&self.db)
    }
}

impl AttributeStore for SqliteAttributes {
    fn put<'a>(
        &'a self,
        entity: &'a EntityRef,
        attributes: &'a Attributes,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let db = self.ready().await?;
            let kind = wire_kind(&entity.kind);
            let mut tx = db.begin().await.map_err(|e| e.to_string())?;
            for (key, value) in attributes {
                sqlx::query(
                    "INSERT INTO entity_attributes (entity_kind, entity_id, key, value)
                     VALUES (?, ?, ?, ?)
                     ON CONFLICT (entity_kind, entity_id, key) DO UPDATE SET value = excluded.value",
                )
                .bind(&kind)
                .bind(&entity.id)
                .bind(key)
                .bind(value)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            tx.commit().await.map_err(|e| e.to_string())
        })
    }

    fn of_kind(
        &self,
        kind: ResourceKind,
    ) -> BoxFuture<'_, Result<BTreeMap<String, Attributes>, String>> {
        Box::pin(async move {
            let db = self.ready().await?;
            let rows = sqlx::query(
                "SELECT entity_id, key, value FROM entity_attributes WHERE entity_kind = ?",
            )
            .bind(wire_kind(&kind))
            .fetch_all(db)
            .await
            .map_err(|e| e.to_string())?;
            let mut found: BTreeMap<String, Attributes> = BTreeMap::new();
            for row in rows {
                found
                    .entry(row.get("entity_id"))
                    .or_default()
                    .insert(row.get("key"), row.get("value"));
            }
            Ok(found)
        })
    }

    fn forget<'a>(&'a self, entity: &'a EntityRef) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let db = self.ready().await?;
            sqlx::query("DELETE FROM entity_attributes WHERE entity_kind = ? AND entity_id = ?")
                .bind(wire_kind(&entity.kind))
                .bind(&entity.id)
                .execute(db)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_merges_reads_by_kind_and_forgets() {
        // One connection: every connection to `:memory:` is its own database.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let store = SqliteAttributes::new(db);
        let run = EntityRef::new(ResourceKind::Run, "r1");
        let attrs = |k: &str, v: &str| Attributes::from([(k.to_string(), v.to_string())]);
        store.put(&run, &attrs("a/k", "1")).await.unwrap();
        store.put(&run, &attrs("a/k", "2")).await.unwrap();
        store.put(&run, &attrs("b/k", "3")).await.unwrap();
        store
            .put(
                &EntityRef::new(ResourceKind::Project, "p1"),
                &attrs("a/k", "4"),
            )
            .await
            .unwrap();

        let runs = store.of_kind(ResourceKind::Run).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs["r1"],
            Attributes::from([
                ("a/k".to_string(), "2".to_string()),
                ("b/k".to_string(), "3".to_string())
            ])
        );
        store.forget(&run).await.unwrap();
        assert!(store.of_kind(ResourceKind::Run).await.unwrap().is_empty());
        assert_eq!(store.of_kind(ResourceKind::Project).await.unwrap().len(), 1);
    }
}
