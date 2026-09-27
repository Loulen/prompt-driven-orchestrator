//! Persistence for provisioning rules: one JSON document per (scope, scope id).

use sqlx::{Row, SqlitePool};

use crate::application::ports::{
    BoxFuture, ProvisioningRulesRepository, StorageError, StoreResult,
};
use crate::domain::provisioning::{ProvisioningRules, ProvisioningScope};

pub(super) async fn init_provisioning_rules(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS provisioning_rules (
            scope TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            rules_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (scope, scope_id)
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

fn scope_key(scope: ProvisioningScope) -> String {
    format!("{scope:?}").to_ascii_lowercase()
}

/// The rules stored for a scope, empty when none are (or they do not parse).
pub async fn load_rules(
    db: &SqlitePool,
    scope: ProvisioningScope,
    scope_id: &str,
) -> Result<ProvisioningRules, sqlx::Error> {
    let row =
        sqlx::query("SELECT rules_json FROM provisioning_rules WHERE scope = ? AND scope_id = ?")
            .bind(scope_key(scope))
            .bind(scope_id)
            .fetch_optional(db)
            .await?;
    Ok(row
        .and_then(|row| serde_json::from_str(&row.get::<String, _>("rules_json")).ok())
        .unwrap_or_default())
}

pub async fn save_rules(
    db: &SqlitePool,
    scope: ProvisioningScope,
    scope_id: &str,
    rules: &ProvisioningRules,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(rules).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        "INSERT INTO provisioning_rules (scope, scope_id, rules_json, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(scope, scope_id) DO UPDATE SET
           rules_json = excluded.rules_json, updated_at = excluded.updated_at",
    )
    .bind(scope_key(scope))
    .bind(scope_id)
    .bind(json)
    .bind(pdo_kernel::clock::now_iso())
    .execute(db)
    .await?;
    Ok(())
}

/// The [`ProvisioningRulesRepository`] over the daemon's SQLite database.
#[derive(Clone)]
pub struct SqliteProvisioningRules {
    db: SqlitePool,
}

impl SqliteProvisioningRules {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl ProvisioningRulesRepository for SqliteProvisioningRules {
    fn load<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
    ) -> BoxFuture<'a, StoreResult<ProvisioningRules>> {
        Box::pin(async move {
            load_rules(&self.db, scope, scope_id)
                .await
                .map_err(|e| StorageError(e.to_string()))
        })
    }

    fn save<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
        rules: &'a ProvisioningRules,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            save_rules(&self.db, scope, scope_id, rules)
                .await
                .map_err(|e| StorageError(e.to_string()))
        })
    }
}
