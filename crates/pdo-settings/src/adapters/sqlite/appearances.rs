//! The Apparence of each Actor over SQLite (`actor_appearances`): one row per
//! Actor, keyed by the Actor's JSON (`{"kind":"identified","namespace":…}`),
//! the document kept as its JSON.

use pdo_kernel::Actor;
use sqlx::{Row, SqlitePool};

use crate::domain::appearance::AppearanceDocument;

/// The key of `actor`'s row: its JSON, whose field order serde fixes.
fn key(actor: &Actor) -> String {
    serde_json::to_string(actor).unwrap_or_else(|_| actor.kind().to_string())
}

/// `actor`'s document; a row no longer read as a document counts as none.
pub async fn get(
    db: &SqlitePool,
    actor: &Actor,
) -> Result<Option<AppearanceDocument>, sqlx::Error> {
    let row = sqlx::query("SELECT document FROM actor_appearances WHERE actor = ?")
        .bind(key(actor))
        .fetch_optional(db)
        .await?;
    Ok(row.and_then(|row| serde_json::from_str(&row.get::<String, _>("document")).ok()))
}

/// Replace `actor`'s document.
pub async fn put(
    db: &SqlitePool,
    actor: &Actor,
    document: &AppearanceDocument,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(document).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    sqlx::query(
        "INSERT INTO actor_appearances (actor, document, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(actor) DO UPDATE SET document = excluded.document, updated_at = excluded.updated_at",
    )
    .bind(key(actor))
    .bind(json)
    .bind(pdo_kernel::clock::now_iso())
    .execute(db)
    .await?;
    Ok(())
}
