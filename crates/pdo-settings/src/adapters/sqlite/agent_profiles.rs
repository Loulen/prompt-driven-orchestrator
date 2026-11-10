//! The agentic profiles over SQLite (`agent_profiles`). Uniqueness of the name
//! is the use case's, so its refusal can name the clashing profile; the
//! case-insensitive unique index is the last line of defence.

use std::collections::BTreeMap;

use pdo_harness::agent_choice::ResolvedCombo;
use sqlx::{Row, SqlitePool};

use crate::domain::agent_profile::{AgentProfile, ProfileDraft, DEFAULT_PROFILE_ID};

fn row_to_profile(row: &sqlx::sqlite::SqliteRow) -> AgentProfile {
    AgentProfile {
        id: row.get("id"),
        name: row.get("name"),
        harness: row.get("harness"),
        model: row
            .get::<Option<String>, _>("model")
            .filter(|s| !s.is_empty()),
        effort: row
            .get::<Option<String>, _>("effort")
            .filter(|s| !s.is_empty()),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        attributes: Default::default(),
    }
}

/// A profile id: `agp-<ts>-<short uuid>`, like a Projet's.
fn generate_profile_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("agp-{ts}-{short}")
}

/// Every profile, [`DEFAULT_PROFILE_ID`] first, then by creation order — a
/// stable listing, so the settings panel does not reshuffle rows on every fetch.
pub async fn list(db: &SqlitePool) -> Result<Vec<AgentProfile>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM agent_profiles ORDER BY (id = ?) DESC, created_at ASC, rowid ASC",
    )
    .bind(DEFAULT_PROFILE_ID)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_profile).collect())
}

pub async fn get(db: &SqlitePool, id: &str) -> Result<Option<AgentProfile>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM agent_profiles WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_profile))
}

/// Every profile's `id → combo`, in ONE query: a resolution walking Node → Run
/// → Projet → instance → Default never mixes two revisions of a profile, and
/// never sees one renamed or edited mid-walk (ADR-0057 ¶4).
pub async fn snapshot(db: &SqlitePool) -> Result<BTreeMap<String, ResolvedCombo>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM agent_profiles")
        .fetch_all(db)
        .await?;
    Ok(rows
        .iter()
        .map(row_to_profile)
        .map(|p| (p.id.clone(), p.combo()))
        .collect())
}

/// Case-insensitive lookup by name, trimmed on both sides.
pub async fn find_by_name(
    db: &SqlitePool,
    name: &str,
) -> Result<Option<AgentProfile>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT * FROM agent_profiles WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(row.as_ref().map(row_to_profile))
}

pub async fn create(db: &SqlitePool, draft: ProfileDraft) -> Result<AgentProfile, sqlx::Error> {
    let id = generate_profile_id();
    let now = pdo_kernel::clock::now_iso();
    sqlx::query(
        "INSERT INTO agent_profiles (id, name, harness, model, effort, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&draft.name)
    .bind(&draft.harness)
    .bind(&draft.model)
    .bind(&draft.effort)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(AgentProfile {
        id,
        name: draft.name,
        harness: draft.harness,
        model: draft.model,
        effort: draft.effort,
        created_at: now.clone(),
        updated_at: now,
        attributes: Default::default(),
    })
}

/// Replace the editable fields of `id` and re-read it — cheap (a primary key
/// lookup), and honest about `created_at`, which an edit never touches.
pub async fn replace(
    db: &SqlitePool,
    id: &str,
    draft: ProfileDraft,
) -> Result<Option<AgentProfile>, sqlx::Error> {
    sqlx::query(
        "UPDATE agent_profiles SET name = ?, harness = ?, model = ?, effort = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&draft.name)
    .bind(&draft.harness)
    .bind(&draft.model)
    .bind(&draft.effort)
    .bind(pdo_kernel::clock::now_iso())
    .bind(id)
    .execute(db)
    .await?;
    get(db, id).await
}

/// Whether a row was removed.
pub async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM agent_profiles WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}
