//! The Combine's writes, flattened on write (ADR-0077 §3):
//!
//! 1. the absorbent leaves whatever absorbed it — the operator just asked it to
//!    carry the series, so it is a top-level row from now on;
//! 2. every member that was itself an absorbent hands its members over;
//! 3. every member now points at the absorbent (a member already absorbed
//!    elsewhere moves: one absorbent per member);
//! 4. the absorbent's name is refreshed on all its rows — it is the name shown
//!    when the absorbent has no Run in the period.
//!
//! A chain A → B → C therefore always ends as C absorbing A and B.

use std::collections::BTreeMap;

use super::{row_of, Row};
use crate::domain::absorption::{AbsorptionRow, Named, Scope, NODE};

pub async fn combine_in(
    tx: &mut sqlx::SqliteConnection,
    scope: Scope<'_>,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    let now = pdo_kernel::clock::now_iso();
    let (dimension, scope_key) = (scope.dimension, scope.key);
    sqlx::query("DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?")
        .bind(dimension)
        .bind(scope_key)
        .bind(&absorbent.key)
        .execute(&mut *tx)
        .await?;
    for member in members.iter().filter(|m| m.key != absorbent.key) {
        sqlx::query(
            "UPDATE stats_absorptions SET absorbent = ?, absorbent_name = ? \
             WHERE dimension = ? AND scope = ? AND absorbent = ?",
        )
        .bind(&absorbent.key)
        .bind(&absorbent.name)
        .bind(dimension)
        .bind(scope_key)
        .bind(&member.key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO stats_absorptions \
               (dimension, scope, scope_name, member, member_name, absorbent, absorbent_name, \
                origin, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (dimension, scope, member) DO UPDATE SET \
               member_name = excluded.member_name, absorbent = excluded.absorbent, \
               absorbent_name = excluded.absorbent_name, origin = excluded.origin, \
               created_at = excluded.created_at",
        )
        .bind(dimension)
        .bind(scope_key)
        .bind(scope.name)
        .bind(&member.key)
        .bind(&member.name)
        .bind(&absorbent.key)
        .bind(&absorbent.name)
        .bind(origin)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE stats_absorptions SET absorbent_name = ? \
         WHERE dimension = ? AND scope = ? AND absorbent = ?",
    )
    .bind(&absorbent.name)
    .bind(dimension)
    .bind(scope_key)
    .bind(&absorbent.key)
    .execute(&mut *tx)
    .await?;
    if !scope.name.is_empty() {
        sqlx::query(
            "UPDATE stats_absorptions SET scope_name = ? WHERE dimension = ? AND scope = ?",
        )
        .bind(scope.name)
        .bind(dimension)
        .bind(scope_key)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

async fn rows_of_scope(
    tx: &mut sqlx::SqliteConnection,
    dimension: &str,
    scope: &str,
) -> Result<Vec<AbsorptionRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT dimension, scope, scope_name, member, member_name, absorbent, absorbent_name, \
                origin, created_at \
         FROM stats_absorptions WHERE dimension = ? AND scope = ? \
         ORDER BY absorbent, created_at, member",
    )
    .bind(dimension)
    .bind(scope)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rows.into_iter().map(row_of).collect())
}

/// Move the Node absorptions scoped to Pipeline `from` under `to`, which just
/// absorbed it. What `to`'s scope already says wins: a Node it already
/// absorbs stays where it is, and a moved absorbent that `to` already absorbs
/// hands over to that absorbent — one level, always.
pub async fn carry_node_absorptions(
    tx: &mut sqlx::SqliteConnection,
    from: &str,
    to: &Named,
) -> Result<(), sqlx::Error> {
    let moved = rows_of_scope(tx, NODE, from).await?;
    if moved.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ?")
        .bind(NODE)
        .bind(from)
        .execute(&mut *tx)
        .await?;
    let mut groups: BTreeMap<(String, String), Vec<(Named, String)>> = BTreeMap::new();
    for row in moved {
        groups
            .entry((row.absorbent.clone(), row.absorbent_name.clone()))
            .or_default()
            .push((
                Named {
                    key: row.member,
                    name: row.member_name,
                },
                row.origin,
            ));
    }
    for ((absorbent_key, absorbent_name), members) in groups {
        let target = rows_of_scope(tx, NODE, &to.key).await?;
        let absorbent = target
            .iter()
            .find(|row| row.member == absorbent_key)
            .map(|row| Named {
                key: row.absorbent.clone(),
                name: row.absorbent_name.clone(),
            })
            .unwrap_or(Named {
                key: absorbent_key,
                name: absorbent_name,
            });
        for (member, origin) in members {
            if member.key == absorbent.key || target.iter().any(|row| row.member == member.key) {
                continue;
            }
            combine_in(
                tx,
                Scope {
                    dimension: NODE,
                    key: &to.key,
                    name: &to.name,
                },
                &absorbent,
                std::slice::from_ref(&member),
                &origin,
            )
            .await?;
        }
    }
    Ok(())
}
