//! Persistence for **profils agentiques** — instance-scoped, named, reusable
//! combinations of a required harness with an optional model and effort
//! (#563, ADR-0057, CONTEXT.md §*Profil agentique*).
//!
//! A profile's identity is its `id`, stable and distinct from its `name`: a
//! rename never breaks a referent, which is why every reference PDO stores
//! (on a node, a Run, a Projet, the instance) is an `id`, never a `name`
//! (ADR-0057 ¶2).
//!
//! Every instance carries a **reserved** profile, [`DEFAULT_PROFILE_ID`], seeded
//! at [`init`] time if absent: named `Default`, harness `claude`, no model, no
//! effort. It is the floor of the whole precedence chain — modifiable and
//! renamable like any other profile, but [`delete`] refuses it unconditionally
//! (an operator must always have a plancher to fall back to, ADR-0057 ¶3).
//!
//! Names are unique **case-insensitively** (ADR-0057 ¶5, #563 AC25) so a picker
//! never shows two entries a human cannot tell apart. Uniqueness is enforced in
//! code (a read-then-decide check under the whole-store discipline every other
//! store here uses — `project_store`'s "at most one Projet per path" is the
//! closest precedent) rather than by a `UNIQUE` index on a computed column,
//! because the message must **name the clashing profile**, which a bare
//! constraint violation cannot.
//!
//! [`snapshot`] is the one-shot, atomic read the spawn seam calls: a single
//! `SELECT *` collapsed into an id → combo map, so a resolution that walks four
//! tiers reads exactly one revision of every profile it might touch — never two
//! (ADR-0057 ¶4: "au spawn, PDO lit et gèle une seule révision complète").

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::fmt;

use crate::agent_choice::ResolvedCombo;

/// The reserved id of the instance's floor profile (ADR-0057 ¶3). Distinct from
/// its `name` (`Default`), which is editable — only the id is fixed forever, so
/// a rename can never orphan the plancher every resolution falls back to.
pub(crate) const DEFAULT_PROFILE_ID: &str = "default";
/// The seeded name of the reserved profile. Editable afterwards like any other
/// profile's name (still subject to the case-insensitive uniqueness check).
pub(crate) const DEFAULT_PROFILE_NAME: &str = "Default";

/// A persisted agent profile: a stable `id`, a case-insensitively unique `name`,
/// a required `harness`, and an optional `model` / `effort` — the same shape a
/// node's `Custom` choice carries inline (ADR-0057 ¶1: "Custom porte la même
/// forme qu'un profil").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentProfile {
    pub id: String,
    pub name: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AgentProfile {
    /// This profile's combination, the shape [`snapshot`] indexes by id.
    pub(crate) fn combo(&self) -> ResolvedCombo {
        ResolvedCombo {
            harness: self.harness.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
        }
    }
}

/// Why a create/update/delete was refused — named so the HTTP layer can turn
/// each variant into the right status code and an actionable message, the same
/// split `sandbox_profile::validate_*` uses (message strings) but as a proper
/// enum here because the caller (delete) also needs to branch on *which* refusal
/// happened, not just render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentProfileError {
    /// The name is blank once trimmed.
    EmptyName,
    /// The harness is blank once trimmed — required (#563 AC2).
    EmptyHarness,
    /// Another profile already carries this name, case-insensitively.
    DuplicateName {
        existing_id: String,
        name: String,
    },
    /// No profile with this id.
    NotFound,
    /// [`DEFAULT_PROFILE_ID`] can never be deleted (ADR-0057 ¶3 / #563 AC10).
    DefaultUndeletable,
    Storage(String),
}

impl fmt::Display for AgentProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a profile name cannot be blank"),
            Self::EmptyHarness => write!(f, "a profile's harness cannot be blank"),
            Self::DuplicateName { name, .. } => write!(
                f,
                "a profile named `{name}` already exists (names are unique \
                 case-insensitively)"
            ),
            Self::NotFound => write!(f, "no such agent profile"),
            Self::DefaultUndeletable => write!(
                f,
                "the `{DEFAULT_PROFILE_NAME}` profile is the instance's reserved \
                 floor and cannot be deleted — rename or edit it instead"
            ),
            Self::Storage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AgentProfileError {}

/// Create the `agent_profiles` table if absent and seed the reserved
/// [`DEFAULT_PROFILE_ID`] row if it is missing. Idempotent — safe on every boot,
/// same idiom as `sandbox_profile::init` / `project_store::init`.
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_profiles (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT NOT NULL,
            model      TEXT,
            effort     TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    let exists = sqlx::query("SELECT 1 FROM agent_profiles WHERE id = ?")
        .bind(DEFAULT_PROFILE_ID)
        .fetch_optional(db)
        .await?
        .is_some();
    if !exists {
        let now = crate::event_log::now_iso();
        sqlx::query(
            "INSERT INTO agent_profiles (id, name, harness, model, effort, created_at, updated_at) \
             VALUES (?, ?, ?, NULL, NULL, ?, ?)",
        )
        .bind(DEFAULT_PROFILE_ID)
        .bind(DEFAULT_PROFILE_NAME)
        .bind(crate::harness_registry::CLAUDE)
        .bind(&now)
        .bind(&now)
        .execute(db)
        .await?;
    }
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_profiles_name_nocase \
         ON agent_profiles(name COLLATE NOCASE)",
    )
    .execute(db)
    .await?;

    Ok(())
}

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
    }
}

/// Generate a profile id (`agp-<ts>-<short uuid>`), mirroring
/// `project_store::generate_project_id`.
fn generate_profile_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("agp-{ts}-{short}")
}

/// All profiles, [`DEFAULT_PROFILE_ID`] first, then by creation order — a stable
/// listing so the settings panel does not reshuffle rows on every fetch.
pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<AgentProfile>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM agent_profiles ORDER BY (id = ?) DESC, created_at ASC, rowid ASC",
    )
    .bind(DEFAULT_PROFILE_ID)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_profile).collect())
}

/// One profile by id, or `None`.
pub(crate) async fn get(db: &SqlitePool, id: &str) -> Result<Option<AgentProfile>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM agent_profiles WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_profile))
}

/// The atomic snapshot the spawn seam (and the pure resolver in
/// [`crate::agent_choice`]) reads: every profile's `id → combo`, in ONE query —
/// so a resolution walking Node → Run → Projet → instance → Default never mixes
/// two revisions of the same profile, and never sees a profile renamed or edited
/// mid-walk (ADR-0057 ¶4).
pub(crate) async fn snapshot(
    db: &SqlitePool,
) -> Result<BTreeMap<String, ResolvedCombo>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM agent_profiles")
        .fetch_all(db)
        .await?;
    Ok(rows
        .iter()
        .map(row_to_profile)
        .map(|p| (p.id.clone(), p.combo()))
        .collect())
}

/// Case-insensitive lookup by name, trimmed on both sides — the basis of the
/// uniqueness check ([`create`] / [`update`]) and handy for the HTTP layer to
/// resolve a name back to an id without a second round trip.
pub(crate) async fn find_by_name_ci(
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

/// Trim `name`/`harness`, reject blanks, reject a case-insensitive clash with
/// another profile (`excluding_id` lets [`update`] ignore the row being edited).
/// Returns the normalised `(name, harness)` on success.
async fn validate_name_and_harness(
    db: &SqlitePool,
    name: &str,
    harness: &str,
    excluding_id: Option<&str>,
) -> Result<(String, String), AgentProfileError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AgentProfileError::EmptyName);
    }
    let harness = harness.trim().to_string();
    if harness.is_empty() {
        return Err(AgentProfileError::EmptyHarness);
    }
    if let Some(existing) = find_by_name_ci(db, &name)
        .await
        .map_err(|_| AgentProfileError::NotFound)?
    {
        if Some(existing.id.as_str()) != excluding_id {
            return Err(AgentProfileError::DuplicateName {
                existing_id: existing.id,
                name: existing.name,
            });
        }
    }
    Ok((name, harness))
}

/// Create a new profile. `model` / `effort` are normalised: an empty string is
/// stored as `NULL` (unset) — the same `Some("")`-is-unset discipline every
/// other free-text column in this daemon follows (#347).
pub(crate) async fn create(
    db: &SqlitePool,
    name: &str,
    harness: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<AgentProfile, AgentProfileError> {
    let (name, harness) = validate_name_and_harness(db, name, harness, None).await?;
    let id = generate_profile_id();
    let now = crate::event_log::now_iso();
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    let effort = effort.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query(
        "INSERT INTO agent_profiles (id, name, harness, model, effort, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&harness)
    .bind(model)
    .bind(effort)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await
    .map_err(|error| AgentProfileError::Storage(error.to_string()))?;
    Ok(AgentProfile {
        id,
        name,
        harness,
        model: model.map(String::from),
        effort: effort.map(String::from),
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Edit (and/or rename) an existing profile, [`DEFAULT_PROFILE_ID`] included
/// (ADR-0057 ¶3: it "reste modifiable et renommable"). All three of
/// `name`/`harness`/`model`/`effort` are supplied wholesale — this is a full
/// replace of the row's editable fields, not a sparse patch, mirroring how
/// `Custom` and a profile share one complete-combination shape.
pub(crate) async fn update(
    db: &SqlitePool,
    id: &str,
    name: &str,
    harness: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<AgentProfile, AgentProfileError> {
    if get(db, id)
        .await
        .map_err(|_| AgentProfileError::NotFound)?
        .is_none()
    {
        return Err(AgentProfileError::NotFound);
    }
    let (name, harness) = validate_name_and_harness(db, name, harness, Some(id)).await?;
    let now = crate::event_log::now_iso();
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    let effort = effort.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query(
        "UPDATE agent_profiles SET name = ?, harness = ?, model = ?, effort = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&harness)
    .bind(model)
    .bind(effort)
    .bind(&now)
    .bind(id)
    .execute(db)
    .await
    .map_err(|error| AgentProfileError::Storage(error.to_string()))?;
    // Re-fetch rather than reconstruct: cheap (PK lookup) and it keeps this
    // function honest about `created_at`, which it never touches.
    get(db, id)
        .await
        .map_err(|_| AgentProfileError::NotFound)?
        .ok_or(AgentProfileError::NotFound)
}

/// Delete a profile. [`DEFAULT_PROFILE_ID`] is refused unconditionally
/// (ADR-0057 ¶3 / #563 AC10) — checked BEFORE any query, so the reserved
/// profile is never even looked up. Otherwise **unconditional**, same posture as
/// `sandbox_profile::delete` (ADR-0031 §7): no referential integrity in the
/// database, the referents dialog is what informs the confirmation, not a
/// database-level refusal.
pub(crate) async fn delete(db: &SqlitePool, id: &str) -> Result<bool, AgentProfileError> {
    if id == DEFAULT_PROFILE_ID {
        return Err(AgentProfileError::DefaultUndeletable);
    }
    let res = sqlx::query("DELETE FROM agent_profiles WHERE id = ?")
        .bind(id)
        .execute(db)
        .await
        .map_err(|_| AgentProfileError::NotFound)?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_db() -> SqlitePool {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn init_is_idempotent_and_seeds_default_exactly_once() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        let all = list(&db).await.unwrap();
        assert_eq!(all.len(), 1, "Default must be seeded exactly once");
        let d = &all[0];
        assert_eq!(d.id, DEFAULT_PROFILE_ID);
        assert_eq!(d.name, DEFAULT_PROFILE_NAME);
        assert_eq!(d.harness, crate::harness_registry::CLAUDE);
        assert_eq!(d.model, None);
        assert_eq!(d.effort, None);
    }

    #[tokio::test]
    async fn default_profile_is_immediately_resolvable() {
        // AC10: Default toujours disponible dès l'instanciation.
        let db = mem_db().await;
        let d = get(&db, DEFAULT_PROFILE_ID).await.unwrap().unwrap();
        assert_eq!(d.combo().harness, crate::harness_registry::CLAUDE);
    }

    #[tokio::test]
    async fn create_a_named_profile() {
        let db = mem_db().await;
        let p = create(&db, "Fast Reviewer", "claude", Some("opus"), Some("high"))
            .await
            .unwrap();
        assert_eq!(p.name, "Fast Reviewer");
        assert_eq!(p.harness, "claude");
        assert_eq!(p.model.as_deref(), Some("opus"));
        assert_eq!(p.effort.as_deref(), Some("high"));
        assert_ne!(p.id, DEFAULT_PROFILE_ID);
        assert!(get(&db, &p.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn empty_name_or_harness_is_refused() {
        let db = mem_db().await;
        assert_eq!(
            create(&db, "  ", "claude", None, None).await.unwrap_err(),
            AgentProfileError::EmptyName
        );
        assert_eq!(
            create(&db, "X", "  ", None, None).await.unwrap_err(),
            AgentProfileError::EmptyHarness
        );
    }

    #[tokio::test]
    async fn names_are_unique_case_insensitively() {
        // AC25.
        let db = mem_db().await;
        create(&db, "Reviewer", "claude", None, None).await.unwrap();
        let err = create(&db, "REVIEWER", "opencode", None, None)
            .await
            .unwrap_err();
        match err {
            AgentProfileError::DuplicateName { name, .. } => assert_eq!(name, "Reviewer"),
            other => panic!("expected DuplicateName, got {other:?}"),
        }
        // Also clashes with the reserved Default name, case-insensitively.
        let err2 = create(&db, "default", "claude", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err2, AgentProfileError::DuplicateName { .. }));
    }

    #[tokio::test]
    async fn rename_preserves_id_and_referents_would_still_resolve() {
        // ADR-0057 ¶2: l'identité ne dépend pas du nom.
        let db = mem_db().await;
        let p = create(&db, "Old Name", "claude", None, None).await.unwrap();
        let updated = update(&db, &p.id, "New Name", "claude", None, None)
            .await
            .unwrap();
        assert_eq!(updated.id, p.id);
        assert_eq!(updated.name, "New Name");
        assert_eq!(get(&db, &p.id).await.unwrap().unwrap().name, "New Name");
    }

    #[tokio::test]
    async fn rename_to_an_existing_name_is_refused() {
        let db = mem_db().await;
        let a = create(&db, "Alpha", "claude", None, None).await.unwrap();
        create(&db, "Bravo", "claude", None, None).await.unwrap();
        let err = update(&db, &a.id, "bravo", "claude", None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, AgentProfileError::DuplicateName { .. }));
    }

    #[tokio::test]
    async fn rename_to_its_own_name_case_changed_is_allowed() {
        // Excluding self from the clash check: editing "Alpha" to "ALPHA" must not
        // spuriously collide with itself.
        let db = mem_db().await;
        let a = create(&db, "Alpha", "claude", None, None).await.unwrap();
        let updated = update(&db, &a.id, "ALPHA", "claude", None, None)
            .await
            .unwrap();
        assert_eq!(updated.name, "ALPHA");
    }

    #[tokio::test]
    async fn default_is_editable_and_renamable() {
        // AC: Default reste modifiable et renommable.
        let db = mem_db().await;
        let updated = update(
            &db,
            DEFAULT_PROFILE_ID,
            "House Default",
            "opencode",
            Some("gpt"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.id, DEFAULT_PROFILE_ID);
        assert_eq!(updated.name, "House Default");
        assert_eq!(updated.harness, "opencode");
        assert_eq!(updated.model.as_deref(), Some("gpt"));
    }

    #[tokio::test]
    async fn default_cannot_be_deleted() {
        // AC10/AC12.
        let db = mem_db().await;
        let err = delete(&db, DEFAULT_PROFILE_ID).await.unwrap_err();
        assert_eq!(err, AgentProfileError::DefaultUndeletable);
        assert!(get(&db, DEFAULT_PROFILE_ID).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_named_profile_can_be_deleted() {
        let db = mem_db().await;
        let p = create(&db, "Temp", "claude", None, None).await.unwrap();
        assert!(delete(&db, &p.id).await.unwrap());
        assert!(get(&db, &p.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deleting_an_unknown_id_is_a_noop_false() {
        let db = mem_db().await;
        assert!(!delete(&db, "agp-nope").await.unwrap());
    }

    #[tokio::test]
    async fn update_of_unknown_id_is_not_found() {
        let db = mem_db().await;
        let err = update(&db, "agp-nope", "X", "claude", None, None)
            .await
            .unwrap_err();
        assert_eq!(err, AgentProfileError::NotFound);
    }

    #[tokio::test]
    async fn empty_model_and_effort_normalise_to_none() {
        let db = mem_db().await;
        let p = create(&db, "P", "claude", Some(""), Some("  "))
            .await
            .unwrap();
        assert_eq!(p.model, None);
        assert_eq!(p.effort, None);
    }

    #[tokio::test]
    async fn snapshot_is_one_atomic_read_of_every_profile() {
        // ADR-0057 ¶4: the spawn seam reads one full revision in one shot.
        let db = mem_db().await;
        let a = create(&db, "Alpha", "claude", Some("opus"), Some("high"))
            .await
            .unwrap();
        let b = create(&db, "Bravo", "opencode", None, None).await.unwrap();
        let snap = snapshot(&db).await.unwrap();
        assert_eq!(snap.len(), 3, "Default + Alpha + Bravo");
        assert_eq!(
            snap[DEFAULT_PROFILE_ID].harness,
            crate::harness_registry::CLAUDE
        );
        assert_eq!(snap[&a.id].harness, "claude");
        assert_eq!(snap[&a.id].model.as_deref(), Some("opus"));
        assert_eq!(snap[&b.id].harness, "opencode");
        assert_eq!(snap[&b.id].model, None);
    }

    #[tokio::test]
    async fn list_orders_default_first_then_creation_order() {
        let db = mem_db().await;
        let a = create(&db, "Alpha", "claude", None, None).await.unwrap();
        let b = create(&db, "Bravo", "claude", None, None).await.unwrap();
        let names: Vec<String> = list(&db).await.unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(names, vec![DEFAULT_PROFILE_ID.to_string(), a.id, b.id]);
    }

    #[tokio::test]
    async fn find_by_name_ci_matches_regardless_of_case_and_whitespace() {
        let db = mem_db().await;
        let p = create(&db, "Fast Reviewer", "claude", None, None)
            .await
            .unwrap();
        let found = find_by_name_ci(&db, "  fast REVIEWER  ").await.unwrap();
        assert_eq!(found.unwrap().id, p.id);
        assert!(find_by_name_ci(&db, "nope").await.unwrap().is_none());
    }
}
