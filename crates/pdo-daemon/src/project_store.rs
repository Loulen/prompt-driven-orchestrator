//! Persistence for **Projets** — a named grouping of member repository paths,
//! and the middle tier of the harness precedence axis (ADR-0046, #552).
//!
//! A Projet is materialised **on demand**: no row exists until a human names a
//! group or attaches a setting (ADR-0046, same posture as the price table of
//! ADR-0034 — nothing is ever seeded). Membership is a **verbatim** path
//! comparison — a repo path is never canonicalised (ADR-0033), so two spellings
//! of the same path are two paths and nothing reconciles them silently. A path
//! belongs to **at most one** Projet; a second attach is a **refusal that names
//! the owning Projet**, rendered before any write.
//!
//! The Projet of a Run is the one that owns its **primary** repo path; a
//! secondary repo (ADR-0042) is never consulted here, so adding or removing one
//! can change neither the Projet nor the resolved harness. That property lives at
//! the spawn seam (which passes the primary path only) — this store just answers
//! "who owns this exact path".
//!
//! It also stores **Pipeline validation** (CONTEXT.md *Pipeline validé*, #974):
//! for which Projets a Pipeline is offered at launch. A Projet-side datum of the
//! instance, never written into the YAML, so an export carries no Projet. **No
//! row means "all Projets"** — that is what keeps the Pipelines present before
//! the feature visible without a migration; what PDO creates, duplicates or
//! imports is written "not validated" (a *Pipeline de test*) by the caller.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// A persisted Projet: a named group of member paths and the optional settings it
/// carries. Each optional setting is `None` when the Projet states none — the
/// tier is then transparent to resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    /// The harness this Projet carries (ADR-0046). An empty string is treated as
    /// "unset" by the resolver seam (the `Some("")` trap of #347).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The **project** tier of [`crate::auto_fail::resolve_auto_fail`] (ADR-0049).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fail: Option<bool>,
    /// The Projet tier of the agentic-profile union (#563, ADR-0057). When set it
    /// wins outright at this tier over [`Self::harness`] (never merges).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_choice: Option<crate::agent_choice::AgentChoice>,
    /// The Projet tier of the **skills** selection (#669, ADR-0062): unioned with
    /// the instance, Run and Node tiers by `skill_selection::resolve` for every
    /// Run whose primary repo this Projet owns. Empty ⇒ transparent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<crate::skill_selection::SkillRef>,
    /// Member repository paths, compared **verbatim** (ADR-0033). Ordered by
    /// attach time (the `rowid` of `project_members`).
    #[serde(default)]
    pub members: Vec<String>,
}

/// The outcome of attaching a path to a Projet — the "at most one Projet per
/// path" invariant expressed as data so the API can render the named refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AddMember {
    Added,
    /// Already a member of **this** Projet — an idempotent no-op.
    AlreadyMember,
    /// Already a member of **another** Projet. No write happened; the owning
    /// Projet is named so the caller can say which one.
    Refused {
        owner_id: String,
        owner_name: String,
    },
}

/// For which Projets a Pipeline is offered at launch (CONTEXT.md *Pipeline
/// validé*). The wire shape is tagged: `{"kind":"all"}`, `{"kind":"none"}`,
/// `{"kind":"projects","project_ids":[…]}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PipelineValidation {
    /// Every Projet, current and future — also what an absent row reads as.
    #[default]
    #[serde(rename = "all")]
    AllProjects,
    /// A *Pipeline de test*: offered only behind « Show test pipelines ».
    #[serde(rename = "none")]
    NotValidated,
    /// These Projets only, in the order they were validated. Never empty once
    /// stored: [`set_pipeline_validation`] folds an empty list to `NotValidated`.
    Projects { project_ids: Vec<String> },
}

const VALIDATION_ALL: &str = "all";
const VALIDATION_NONE: &str = "none";
const VALIDATION_PROJECTS: &str = "projects";

pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT,
            auto_fail  INTEGER,
            agent_choice TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Pre-ADR-0049 databases have no `auto_fail` column. Guarded `ADD COLUMN`,
    // NULLABLE so an existing Projet states no preference.
    let has_auto_fail =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'auto_fail'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_auto_fail {
        sqlx::query("ALTER TABLE projects ADD COLUMN auto_fail INTEGER")
            .execute(db)
            .await?;
    }

    // Pre-#563 databases have no `agent_choice` column. Guarded `ADD COLUMN`,
    // NULLABLE so an existing Projet falls back to its legacy `harness` column.
    let has_agent_choice =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'agent_choice'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_agent_choice {
        sqlx::query("ALTER TABLE projects ADD COLUMN agent_choice TEXT")
            .execute(db)
            .await?;
    }

    // Pre-#669 databases have no `skills` column. Guarded `ADD COLUMN`, NULLABLE
    // so an existing Projet selects nothing.
    let has_skills =
        sqlx::query("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'skills'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_skills {
        sqlx::query("ALTER TABLE projects ADD COLUMN skills TEXT")
            .execute(db)
            .await?;
    }

    // `path` is the PRIMARY KEY: at-most-one-Projet-per-path is a schema
    // invariant. `project_id` is indexed for the members-of lookup. No FK
    // ON DELETE cascade — deletes clear members explicitly (portable, and the
    // daemon never enables `PRAGMA foreign_keys`).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS project_members (
            path       TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_project_members_project ON project_members(project_id)",
    )
    .execute(db)
    .await?;

    // #974: Pipeline validation. One row per Pipeline that states one (absent ⇒
    // all Projets); the Projets of a `projects` row live in the side table,
    // ordered by `rowid`. Keyed by the Pipeline id (its file stem), which a
    // rename moves explicitly.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_validation (
            pipeline_id TEXT PRIMARY KEY,
            kind        TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pipeline_validation_projects (
            pipeline_id TEXT NOT NULL,
            project_id  TEXT NOT NULL,
            PRIMARY KEY (pipeline_id, project_id)
        )",
    )
    .execute(db)
    .await?;

    Ok(())
}

pub(crate) fn generate_project_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("prj-{ts}-{short}")
}

fn row_to_project(row: &sqlx::sqlite::SqliteRow, members: Vec<String>) -> Project {
    Project {
        id: row.get("id"),
        name: row.get("name"),
        harness: row
            .get::<Option<String>, _>("harness")
            .filter(|s| !s.is_empty()),
        auto_fail: row.get::<Option<i64>, _>("auto_fail").map(|v| v != 0),
        // An unparseable stored choice degrades to `None` (transparent tier)
        // rather than failing the whole read.
        agent_choice: row
            .get::<Option<String>, _>("agent_choice")
            .and_then(|s| serde_json::from_str(&s).ok()),
        // #669: NULL / unparseable ⇒ empty selection (transparent tier).
        skills: crate::skill_selection::from_stored_json(
            row.try_get::<Option<String>, _>("skills").unwrap_or(None),
        ),
        members,
    }
}

async fn members_of(db: &SqlitePool, project_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT path FROM project_members WHERE project_id = ? ORDER BY rowid ASC")
            .bind(project_id)
            .fetch_all(db)
            .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("path")).collect())
}

/// Materialisation on naming (ADR-0046): the only path that creates a `projects`
/// row from a bare name.
pub(crate) async fn create(db: &SqlitePool, name: &str) -> Result<Project, sqlx::Error> {
    let id = generate_project_id();
    let now = crate::event_log::now_iso();
    sqlx::query("INSERT INTO projects (id, name, harness, created_at) VALUES (?, ?, NULL, ?)")
        .bind(&id)
        .bind(name)
        .bind(&now)
        .execute(db)
        .await?;
    Ok(Project {
        id,
        name: name.to_string(),
        harness: None,
        auto_fail: None,
        agent_choice: None,
        skills: Vec::new(),
        members: Vec::new(),
    })
}

pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<Project>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM projects ORDER BY created_at DESC")
        .fetch_all(db)
        .await?;
    let mut projects = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.get("id");
        let members = members_of(db, &id).await?;
        projects.push(row_to_project(row, members));
    }
    Ok(projects)
}

pub(crate) async fn get(db: &SqlitePool, id: &str) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    match row {
        Some(row) => {
            let members = members_of(db, id).await?;
            Ok(Some(row_to_project(&row, members)))
        }
        None => Ok(None),
    }
}

pub(crate) async fn rename(db: &SqlitePool, id: &str, name: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// An empty string is stored as `NULL` — a blank never carries a harness (the
/// `Some("")` trap of #347), matching the resolver's floor-through.
pub(crate) async fn set_harness(
    db: &SqlitePool,
    id: &str,
    harness: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let stored = harness.filter(|s| !s.is_empty());
    let res = sqlx::query("UPDATE projects SET harness = ? WHERE id = ?")
        .bind(stored)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// `None` clears the preference back to "states none" (SQL `NULL`), which is
/// distinct from a stored `false`.
pub(crate) async fn set_auto_fail(
    db: &SqlitePool,
    id: &str,
    auto_fail: Option<bool>,
) -> Result<bool, sqlx::Error> {
    let stored: Option<i64> = auto_fail.map(|v| if v { 1 } else { 0 });
    let res = sqlx::query("UPDATE projects SET auto_fail = ? WHERE id = ?")
        .bind(stored)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// The `project` tier of [`crate::auto_fail::resolve_auto_fail`]. `None` ⇒ no
/// Projet owns the path, or it states no preference — transparent either way.
pub(crate) async fn auto_fail_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<bool>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.auto_fail))
}

/// `None` clears the choice (SQL `NULL`) — the tier goes transparent and the
/// legacy [`Project::harness`] applies again.
pub(crate) async fn set_agent_choice(
    db: &SqlitePool,
    id: &str,
    agent_choice: Option<crate::agent_choice::AgentChoice>,
) -> Result<bool, sqlx::Error> {
    let stored = agent_choice.map(|c| serde_json::to_string(&c).unwrap_or_default());
    let res = sqlx::query("UPDATE projects SET agent_choice = ? WHERE id = ?")
        .bind(stored)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Replace the Projet's skills selection wholesale (#669). An empty list clears
/// to SQL `NULL` — the tier goes transparent.
pub(crate) async fn set_skills(
    db: &SqlitePool,
    id: &str,
    skills: Vec<crate::skill_selection::SkillRef>,
) -> Result<bool, sqlx::Error> {
    let stored = crate::skill_selection::to_stored_json(&crate::skill_selection::normalise(skills));
    let res = sqlx::query("UPDATE projects SET skills = ? WHERE id = ?")
        .bind(stored)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// The Projet tier of `skill_selection::resolve`: the skills selected by the
/// Projet owning `path`, or empty when no Projet owns it.
pub(crate) async fn skills_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Vec<crate::skill_selection::SkillRef>, sqlx::Error> {
    Ok(owner_of(db, path)
        .await?
        .map(|p| p.skills)
        .unwrap_or_default())
}

/// The Projet tier of [`crate::agent_choice::resolve`]. `None` ⇒ transparent —
/// the legacy `harness` signal ([`harness_for_path`]) then applies.
pub(crate) async fn agent_choice_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<crate::agent_choice::AgentChoice>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.agent_choice))
}

/// The Projet that owns `path`, compared **verbatim** (ADR-0033).
pub(crate) async fn owner_of(db: &SqlitePool, path: &str) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query("SELECT project_id FROM project_members WHERE path = ?")
        .bind(path)
        .fetch_optional(db)
        .await?;
    match row {
        Some(row) => get(db, &row.get::<String, _>("project_id")).await,
        None => Ok(None),
    }
}

/// Enforces at-most-one membership **before any write**: a path owned by another
/// Projet is refused (naming the owner) with no partial effect.
pub(crate) async fn add_member(
    db: &SqlitePool,
    project_id: &str,
    path: &str,
) -> Result<AddMember, sqlx::Error> {
    if let Some(owner) = owner_of(db, path).await? {
        if owner.id == project_id {
            return Ok(AddMember::AlreadyMember);
        }
        return Ok(AddMember::Refused {
            owner_id: owner.id,
            owner_name: owner.name,
        });
    }
    let now = crate::event_log::now_iso();
    sqlx::query("INSERT INTO project_members (path, project_id, created_at) VALUES (?, ?, ?)")
        .bind(path)
        .bind(project_id)
        .bind(&now)
        .execute(db)
        .await?;
    Ok(AddMember::Added)
}

/// Detaches from **any** Projet — no `project_id` needed, the path is globally
/// unique.
pub(crate) async fn remove_member(db: &SqlitePool, path: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM project_members WHERE path = ?")
        .bind(path)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Deletes the Projet and all its memberships, freeing those paths.
pub(crate) async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM project_members WHERE project_id = ?")
        .bind(id)
        .execute(db)
        .await?;
    // #974: a Pipeline validated for this Projet alone has nobody left to be
    // offered to — it becomes a Pipeline de test rather than silently "all".
    let orphaned =
        sqlx::query("SELECT pipeline_id FROM pipeline_validation_projects WHERE project_id = ?")
            .bind(id)
            .fetch_all(db)
            .await?;
    sqlx::query("DELETE FROM pipeline_validation_projects WHERE project_id = ?")
        .bind(id)
        .execute(db)
        .await?;
    for row in &orphaned {
        let pipeline_id: String = row.get("pipeline_id");
        if validation_projects_of(db, &pipeline_id).await?.is_empty() {
            set_pipeline_validation(db, &pipeline_id, PipelineValidation::NotValidated).await?;
        }
    }
    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// The `project` tier of [`crate::harness_resolver`]. An empty stored harness is
/// already normalised to `None` by [`owner_of`] → [`get`].
pub(crate) async fn harness_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<String>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.harness))
}

async fn validation_projects_of(
    db: &SqlitePool,
    pipeline_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT project_id FROM pipeline_validation_projects WHERE pipeline_id = ? ORDER BY rowid ASC",
    )
    .bind(pipeline_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("project_id"))
        .collect())
}

fn validation_from_row(kind: &str, project_ids: Vec<String>) -> PipelineValidation {
    match kind {
        VALIDATION_NONE => PipelineValidation::NotValidated,
        VALIDATION_PROJECTS if project_ids.is_empty() => PipelineValidation::NotValidated,
        VALIDATION_PROJECTS => PipelineValidation::Projects { project_ids },
        // `all`, or a kind this build does not know: the permissive reading,
        // the same as an absent row.
        _ => PipelineValidation::AllProjects,
    }
}

/// The validation of one Pipeline. No row ⇒ [`PipelineValidation::AllProjects`].
/// The HTTP surface reads them all at once ([`pipeline_validations`]); this one is
/// the store's own unit of truth for its tests.
#[cfg(test)]
pub(crate) async fn pipeline_validation(
    db: &SqlitePool,
    pipeline_id: &str,
) -> Result<PipelineValidation, sqlx::Error> {
    let row = sqlx::query("SELECT kind FROM pipeline_validation WHERE pipeline_id = ?")
        .bind(pipeline_id)
        .fetch_optional(db)
        .await?;
    match row {
        None => Ok(PipelineValidation::AllProjects),
        Some(row) => {
            let kind: String = row.get("kind");
            let ids = validation_projects_of(db, pipeline_id).await?;
            Ok(validation_from_row(&kind, ids))
        }
    }
}

/// Every stated validation, keyed by Pipeline id — for the list. A Pipeline
/// absent from the map is validated for all Projets.
pub(crate) async fn pipeline_validations(
    db: &SqlitePool,
) -> Result<std::collections::HashMap<String, PipelineValidation>, sqlx::Error> {
    let mut projects: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for row in sqlx::query(
        "SELECT pipeline_id, project_id FROM pipeline_validation_projects ORDER BY rowid ASC",
    )
    .fetch_all(db)
    .await?
    {
        projects
            .entry(row.get("pipeline_id"))
            .or_default()
            .push(row.get("project_id"));
    }
    let mut out = std::collections::HashMap::new();
    for row in sqlx::query("SELECT pipeline_id, kind FROM pipeline_validation")
        .fetch_all(db)
        .await?
    {
        let id: String = row.get("pipeline_id");
        let kind: String = row.get("kind");
        let ids = projects.remove(&id).unwrap_or_default();
        out.insert(id, validation_from_row(&kind, ids));
    }
    Ok(out)
}

/// Replace a Pipeline's validation wholesale. An empty Projet list is stored as
/// [`PipelineValidation::NotValidated`] (« tout décocher = non validé »);
/// duplicates collapse, first occurrence wins. Returns what was stored.
pub(crate) async fn set_pipeline_validation(
    db: &SqlitePool,
    pipeline_id: &str,
    validation: PipelineValidation,
) -> Result<PipelineValidation, sqlx::Error> {
    let validation = match validation {
        PipelineValidation::Projects { project_ids } => {
            let mut seen = std::collections::HashSet::new();
            let ids: Vec<String> = project_ids
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .collect();
            if ids.is_empty() {
                PipelineValidation::NotValidated
            } else {
                PipelineValidation::Projects { project_ids: ids }
            }
        }
        other => other,
    };
    let kind = match &validation {
        PipelineValidation::AllProjects => VALIDATION_ALL,
        PipelineValidation::NotValidated => VALIDATION_NONE,
        PipelineValidation::Projects { .. } => VALIDATION_PROJECTS,
    };
    let now = crate::event_log::now_iso();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM pipeline_validation_projects WHERE pipeline_id = ?")
        .bind(pipeline_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO pipeline_validation (pipeline_id, kind, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(pipeline_id) DO UPDATE SET kind = excluded.kind, updated_at = excluded.updated_at",
    )
    .bind(pipeline_id)
    .bind(kind)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    if let PipelineValidation::Projects { project_ids } = &validation {
        for project_id in project_ids {
            sqlx::query(
                "INSERT INTO pipeline_validation_projects (pipeline_id, project_id) VALUES (?, ?)",
            )
            .bind(pipeline_id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(validation)
}

/// A renamed Pipeline keeps its validation: its rows move to the new id. Any
/// stale rows already under the new id are replaced.
pub(crate) async fn rename_pipeline_validation(
    db: &SqlitePool,
    old_id: &str,
    new_id: &str,
) -> Result<(), sqlx::Error> {
    if old_id == new_id {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    for table in ["pipeline_validation", "pipeline_validation_projects"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE pipeline_id = ?"))
            .bind(new_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!(
            "UPDATE {table} SET pipeline_id = ? WHERE pipeline_id = ?"
        ))
        .bind(new_id)
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// A deleted Pipeline takes its validation with it.
pub(crate) async fn forget_pipeline_validation(
    db: &SqlitePool,
    pipeline_id: &str,
) -> Result<(), sqlx::Error> {
    for table in ["pipeline_validation", "pipeline_validation_projects"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE pipeline_id = ?"))
            .bind(pipeline_id)
            .execute(db)
            .await?;
    }
    Ok(())
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
    async fn init_is_idempotent() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        // A project created before a later init survives it.
        let p = create(&db, "P").await.unwrap();
        init(&db).await.unwrap();
        assert!(get(&db, &p.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn no_row_exists_before_naming() {
        let db = mem_db().await;
        assert!(list(&db).await.unwrap().is_empty());
        assert!(owner_of(&db, "/repos/front").await.unwrap().is_none());
        assert!(harness_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_path_cannot_belong_to_two_projects_refusal_names_the_owner() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        let b = create(&db, "Bravo").await.unwrap();

        assert_eq!(
            add_member(&db, &a.id, "/repos/front").await.unwrap(),
            AddMember::Added
        );
        match add_member(&db, &b.id, "/repos/front").await.unwrap() {
            AddMember::Refused {
                owner_id,
                owner_name,
            } => {
                assert_eq!(owner_id, a.id);
                assert_eq!(owner_name, "Alpha");
            }
            other => panic!("expected refusal naming the owner, got {other:?}"),
        }
        // …and BEFORE any effect: Bravo gained no member.
        assert!(get(&db, &b.id).await.unwrap().unwrap().members.is_empty());
        assert_eq!(
            owner_of(&db, "/repos/front").await.unwrap().unwrap().id,
            a.id
        );
    }

    #[tokio::test]
    async fn re_attaching_to_the_same_project_is_an_idempotent_noop() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        assert_eq!(
            add_member(&db, &a.id, "/repos/front").await.unwrap(),
            AddMember::Added
        );
        assert_eq!(
            add_member(&db, &a.id, "/repos/front").await.unwrap(),
            AddMember::AlreadyMember
        );
        assert_eq!(
            get(&db, &a.id).await.unwrap().unwrap().members,
            vec!["/repos/front".to_string()]
        );
    }

    #[tokio::test]
    async fn membership_comparison_is_verbatim() {
        // ADR-0033: two spellings of the same path are two paths — nothing
        // reconciles them.
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        assert!(owner_of(&db, "/repos/front").await.unwrap().is_some());
        assert!(owner_of(&db, "/repos/front/").await.unwrap().is_none());
        assert!(owner_of(&db, "/repos/FRONT").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn harness_for_path_reads_the_owning_projects_harness() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        add_member(&db, &a.id, "/repos/back").await.unwrap();

        assert!(harness_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());
        assert!(set_harness(&db, &a.id, Some("opencode")).await.unwrap());
        assert_eq!(
            harness_for_path(&db, "/repos/front")
                .await
                .unwrap()
                .as_deref(),
            Some("opencode")
        );
        assert_eq!(
            harness_for_path(&db, "/repos/back")
                .await
                .unwrap()
                .as_deref(),
            Some("opencode")
        );
        assert!(harness_for_path(&db, "/repos/other")
            .await
            .unwrap()
            .is_none());
        // An empty string clears the harness (#347), it does not store a blank.
        assert!(set_harness(&db, &a.id, Some("")).await.unwrap());
        assert!(harness_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn agent_choice_for_path_reads_the_owning_projects_choice() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        add_member(&db, &a.id, "/repos/back").await.unwrap();

        assert!(agent_choice_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());

        let choice = crate::agent_choice::AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        assert!(set_agent_choice(&db, &a.id, Some(choice.clone()))
            .await
            .unwrap());
        assert_eq!(
            agent_choice_for_path(&db, "/repos/front").await.unwrap(),
            Some(choice.clone())
        );
        assert_eq!(
            agent_choice_for_path(&db, "/repos/back").await.unwrap(),
            Some(choice)
        );
        assert!(agent_choice_for_path(&db, "/repos/other")
            .await
            .unwrap()
            .is_none());

        assert!(set_agent_choice(&db, &a.id, None).await.unwrap());
        assert!(agent_choice_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());
    }

    /// Setting one never disturbs the other; resolution order between them is
    /// `agent_choice.rs`'s concern, not this store's.
    #[tokio::test]
    async fn agent_choice_and_legacy_harness_coexist_on_the_same_project() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        assert!(set_harness(&db, &a.id, Some("claude")).await.unwrap());

        let choice = crate::agent_choice::AgentChoice::Custom {
            harness: "opencode".to_string(),
            model: Some("gpt-5".to_string()),
            effort: None,
        };
        assert!(set_agent_choice(&db, &a.id, Some(choice.clone()))
            .await
            .unwrap());

        let p = get(&db, &a.id).await.unwrap().unwrap();
        assert_eq!(p.harness.as_deref(), Some("claude"));
        assert_eq!(p.agent_choice, Some(choice));
    }

    #[tokio::test]
    async fn init_migrates_pre_agent_choice_schema() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE projects (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                harness    TEXT,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, name, harness, created_at) \
             VALUES ('prj-1', 'Alpha', 'claude', 'seed')",
        )
        .execute(&db)
        .await
        .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap();

        let p = get(&db, "prj-1").await.unwrap().unwrap();
        assert_eq!(
            p.harness.as_deref(),
            Some("claude"),
            "pre-existing survives"
        );
        assert_eq!(p.agent_choice, None, "new column defaults to NULL");

        let choice = crate::agent_choice::AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        assert!(set_agent_choice(&db, "prj-1", Some(choice.clone()))
            .await
            .unwrap());
        assert_eq!(
            get(&db, "prj-1").await.unwrap().unwrap().agent_choice,
            Some(choice)
        );
    }

    #[tokio::test]
    async fn rename_and_delete() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();

        assert!(rename(&db, &a.id, "Renamed").await.unwrap());
        assert_eq!(get(&db, &a.id).await.unwrap().unwrap().name, "Renamed");

        assert!(delete(&db, &a.id).await.unwrap());
        assert!(get(&db, &a.id).await.unwrap().is_none());
        assert!(owner_of(&db, "/repos/front").await.unwrap().is_none());
        let b = create(&db, "Bravo").await.unwrap();
        assert_eq!(
            add_member(&db, &b.id, "/repos/front").await.unwrap(),
            AddMember::Added
        );
    }

    #[tokio::test]
    async fn remove_member_frees_the_path() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        assert!(remove_member(&db, "/repos/front").await.unwrap());
        assert!(owner_of(&db, "/repos/front").await.unwrap().is_none());
        assert!(!remove_member(&db, "/repos/front").await.unwrap());
    }

    #[tokio::test]
    async fn a_pipeline_without_a_row_is_validated_for_all_projects() {
        let db = mem_db().await;
        assert_eq!(
            pipeline_validation(&db, "legacy").await.unwrap(),
            PipelineValidation::AllProjects
        );
        assert!(pipeline_validations(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_validation_replaces_and_an_empty_list_means_not_validated() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        let b = create(&db, "Bravo").await.unwrap();
        let stored = set_pipeline_validation(
            &db,
            "p",
            PipelineValidation::Projects {
                project_ids: vec![a.id.clone(), b.id.clone(), a.id.clone()],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            stored,
            PipelineValidation::Projects {
                project_ids: vec![a.id.clone(), b.id.clone()]
            }
        );
        assert_eq!(pipeline_validation(&db, "p").await.unwrap(), stored);

        let stored = set_pipeline_validation(
            &db,
            "p",
            PipelineValidation::Projects {
                project_ids: vec![],
            },
        )
        .await
        .unwrap();
        assert_eq!(stored, PipelineValidation::NotValidated);
        assert_eq!(
            pipeline_validations(&db).await.unwrap().get("p"),
            Some(&PipelineValidation::NotValidated)
        );

        set_pipeline_validation(&db, "p", PipelineValidation::AllProjects)
            .await
            .unwrap();
        assert_eq!(
            pipeline_validation(&db, "p").await.unwrap(),
            PipelineValidation::AllProjects
        );
    }

    #[tokio::test]
    async fn rename_moves_the_validation_and_forget_erases_it() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        let v = PipelineValidation::Projects {
            project_ids: vec![a.id.clone()],
        };
        set_pipeline_validation(&db, "old", v.clone())
            .await
            .unwrap();
        rename_pipeline_validation(&db, "old", "new").await.unwrap();
        assert_eq!(pipeline_validation(&db, "new").await.unwrap(), v);
        assert_eq!(
            pipeline_validation(&db, "old").await.unwrap(),
            PipelineValidation::AllProjects
        );
        forget_pipeline_validation(&db, "new").await.unwrap();
        assert!(pipeline_validations(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_the_only_project_turns_the_pipeline_into_a_test_pipeline() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        let b = create(&db, "Bravo").await.unwrap();
        set_pipeline_validation(
            &db,
            "solo",
            PipelineValidation::Projects {
                project_ids: vec![a.id.clone()],
            },
        )
        .await
        .unwrap();
        set_pipeline_validation(
            &db,
            "shared",
            PipelineValidation::Projects {
                project_ids: vec![a.id.clone(), b.id.clone()],
            },
        )
        .await
        .unwrap();
        delete(&db, &a.id).await.unwrap();
        assert_eq!(
            pipeline_validation(&db, "solo").await.unwrap(),
            PipelineValidation::NotValidated
        );
        assert_eq!(
            pipeline_validation(&db, "shared").await.unwrap(),
            PipelineValidation::Projects {
                project_ids: vec![b.id.clone()]
            }
        );
    }

    #[test]
    fn validation_wire_shape_is_tagged_by_kind() {
        assert_eq!(
            serde_json::to_value(PipelineValidation::AllProjects).unwrap(),
            serde_json::json!({ "kind": "all" })
        );
        assert_eq!(
            serde_json::to_value(PipelineValidation::NotValidated).unwrap(),
            serde_json::json!({ "kind": "none" })
        );
        assert_eq!(
            serde_json::to_value(PipelineValidation::Projects {
                project_ids: vec!["prj-1".into()]
            })
            .unwrap(),
            serde_json::json!({ "kind": "projects", "project_ids": ["prj-1"] })
        );
    }
}
