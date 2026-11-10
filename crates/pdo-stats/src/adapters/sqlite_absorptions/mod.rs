//! The absorptions' store: a table of `pdo.db`, instance configuration beside
//! `instance_config` (ADR-0015). One row per member: `(dimension, scope,
//! member)` is unique, so a member has exactly one absorbent. The storage is
//! **flat, one level**: a Combine flattens on write (absorbing an absorbent
//! moves its members over), so the read never resolves anything transitively.

mod combine;

use sqlx::SqlitePool;

use crate::application::ports::{AbsorptionStore, BoxFuture};
use crate::domain::absorption::{AbsorptionRow, Named, Scope, NODE, PIPELINE};

pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    // `scope` is `''` (never NULL) outside a Node absorption, so the primary key
    // really is unique: SQLite treats two NULLs as distinct.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stats_absorptions (
            dimension      TEXT NOT NULL,
            scope          TEXT NOT NULL DEFAULT '',
            member         TEXT NOT NULL,
            member_name    TEXT NOT NULL,
            absorbent      TEXT NOT NULL,
            absorbent_name TEXT NOT NULL,
            origin         TEXT NOT NULL,
            created_at     TEXT NOT NULL,
            PRIMARY KEY (dimension, scope, member)
        )",
    )
    .execute(db)
    .await?;
    // #892: the name of a Node absorption's Pipeline, shown by Settings — never
    // its key. Added idempotently to a table created by #890.
    let has_scope_name = sqlx::query(
        "SELECT 1 FROM pragma_table_info('stats_absorptions') WHERE name = 'scope_name'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_scope_name {
        sqlx::query("ALTER TABLE stats_absorptions ADD COLUMN scope_name TEXT NOT NULL DEFAULT ''")
            .execute(db)
            .await?;
    }
    Ok(())
}

type Row = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

fn row_of(
    (
        dimension,
        scope,
        scope_name,
        member,
        member_name,
        absorbent,
        absorbent_name,
        origin,
        created_at,
    ): Row,
) -> AbsorptionRow {
    AbsorptionRow {
        dimension,
        scope,
        scope_name,
        member,
        member_name,
        absorbent,
        absorbent_name,
        origin,
        created_at,
    }
}

pub async fn list(db: &SqlitePool) -> Result<Vec<AbsorptionRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT dimension, scope, scope_name, member, member_name, absorbent, absorbent_name, \
                origin, created_at \
         FROM stats_absorptions ORDER BY dimension, scope, absorbent, created_at, member",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(row_of).collect())
}

/// The [`AbsorptionStore`] over a pool of `pdo.db`.
pub struct SqliteAbsorptions {
    db: SqlitePool,
}

impl SqliteAbsorptions {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl AbsorptionStore for SqliteAbsorptions {
    fn list(&self) -> BoxFuture<'_, anyhow::Result<Vec<AbsorptionRow>>> {
        Box::pin(async move { Ok(list(&self.db).await?) })
    }

    fn combine<'a>(
        &'a self,
        scope: Scope<'a>,
        absorbent: &'a Named,
        members: &'a [Named],
        origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut tx = self.db.begin().await?;
            combine::combine_in(&mut tx, scope, absorbent, members, origin).await?;
            if scope.dimension == PIPELINE {
                for member in members.iter().filter(|m| m.key != absorbent.key) {
                    combine::carry_node_absorptions(&mut tx, &member.key, absorbent).await?;
                }
            }
            Ok(tx.commit().await?)
        })
    }

    fn combine_frozen<'a>(
        &'a self,
        dimension: &'a str,
        covered: &'a [Named],
        absorbent: &'a Named,
        members: &'a [Named],
        origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut tx = self.db.begin().await?;
            for raw in covered {
                let scope = Scope {
                    dimension,
                    key: &raw.key,
                    name: &raw.name,
                };
                combine::combine_in(&mut tx, scope, absorbent, members, origin).await?;
            }
            Ok(tx.commit().await?)
        })
    }

    fn remove<'a>(
        &'a self,
        dimension: &'a str,
        scope: &'a str,
        member: &'a str,
        released_scope: &'a str,
        released: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut tx = self.db.begin().await?;
            let delete = "DELETE FROM stats_absorptions \
                          WHERE dimension = ? AND scope = ? AND member = ?";
            sqlx::query(delete)
                .bind(dimension)
                .bind(scope)
                .bind(member)
                .execute(&mut *tx)
                .await?;
            for node in released {
                sqlx::query(delete)
                    .bind(NODE)
                    .bind(released_scope)
                    .bind(node)
                    .execute(&mut *tx)
                    .await?;
            }
            Ok(tx.commit().await?)
        })
    }
}
