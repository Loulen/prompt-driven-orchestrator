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
//! Idiom of `trigger_store` / `instance_config`: SQLite, nullable columns for
//! what the Projet carries (its optional `harness`). Two tables:
//!
//! - `projects` — identity (`id`, `name`) plus the optional `harness` it carries
//!   (the first per-Projet setting, ADR-0046).
//! - `project_members` — `path` → `project_id`, with `path` as the PRIMARY KEY so
//!   "at most one Projet per path" is a **schema invariant**, not merely a code
//!   check. The named refusal is still computed in code (a bare PK conflict could
//!   not name the owner).
//!
//! The Projet of a Run is the one that owns its **primary** repo path; a
//! secondary repo (ADR-0042) is never consulted here, so adding or removing one
//! can change neither the Projet nor the resolved harness. That property lives at
//! the spawn seam (which passes the primary path only) — this store just answers
//! "who owns this exact path".

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// A persisted Projet: a named group of member paths and the optional harness it
/// carries. `harness` is `None` when the Projet names no harness (the tier is
/// then transparent to resolution). `members` is the verbatim member-path list,
/// ordered by attach time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    /// The harness this Projet carries (ADR-0046), or `None`. Stored nullable;
    /// there is no env/default tier for a per-Projet harness, so this is the
    /// degenerate `stored → None`. An empty string is treated as "unset" by the
    /// resolver seam (the `Some("")` trap of #347).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The `auto_fail` preference this Projet carries (ADR-0049), or `None` when
    /// it states none — the **project** tier of
    /// [`crate::auto_fail::resolve_auto_fail`]. `Some(true)`/`Some(false)` is a
    /// stored decision; `None` makes the tier transparent (fall through to the
    /// instance default). No env/default tier of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fail: Option<bool>,
    /// Member repository paths, compared **verbatim** (ADR-0033). Ordered by
    /// attach time (the `rowid` of `project_members`).
    #[serde(default)]
    pub members: Vec<String>,
}

/// The outcome of attaching a path to a Projet — the "at most one Projet per
/// path" invariant expressed as data so the API can render the named refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AddMember {
    /// The path was newly attached to the Projet.
    Added,
    /// The path was already a member of **this** Projet — an idempotent no-op.
    AlreadyMember,
    /// The path is already a member of **another** Projet. No write happened; the
    /// owning Projet is named so the caller can say which one (AC: "refus nommant
    /// le Projet propriétaire, avant tout effet").
    Refused {
        owner_id: String,
        owner_name: String,
    },
}

/// Create the `projects` and `project_members` tables if they do not exist.
///
/// Both are brand-new tables (like `sandbox_profiles` / `audit_log`), so a plain
/// `CREATE TABLE IF NOT EXISTS` is the whole migration — natively idempotent,
/// needing no PRAGMA-guarded `ALTER` (there is no earlier shape to migrate from).
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT,
            auto_fail  INTEGER,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Additive migration for pre-résilience databases: the `auto_fail` column is
    // absent on `projects` tables created before ADR-0049 (#552 shipped without
    // it). Guarded `ADD COLUMN` — NULLABLE so an existing Projet states no
    // preference and the tier stays transparent.
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

    Ok(())
}

/// Generate a Projet id (`prj-<ts>-<short uuid>`), mirroring `trigger_store`.
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
        members,
    }
}

/// Load the member paths of one Projet, ordered by attach time.
async fn members_of(db: &SqlitePool, project_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT path FROM project_members WHERE project_id = ? ORDER BY rowid ASC")
            .bind(project_id)
            .fetch_all(db)
            .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("path")).collect())
}

/// Insert a new (empty, harness-less) Projet with the given name, returning the
/// stored row. Materialisation on naming (ADR-0046): this is the only path that
/// creates a `projects` row from a bare name.
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
        members: Vec::new(),
    })
}

/// All Projets with their member lists, newest first.
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

/// One Projet by id, with its members. `None` ⇒ no such Projet.
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

/// Rename a Projet. Returns `true` iff a row was updated.
pub(crate) async fn rename(db: &SqlitePool, id: &str, name: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Set (or clear, with `None`) the harness a Projet carries. Returns `true` iff a
/// row was updated. An empty string is stored as `NULL` — a blank never carries a
/// harness (the `Some("")` trap of #347), matching the resolver's floor-through.
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

/// Set (or clear, with `None`) the `auto_fail` preference a Projet carries
/// (ADR-0049). `Some(true)` stores `1`, `Some(false)` stores `0`, `None` clears
/// it back to "states no preference" (SQL `NULL`). Returns `true` iff a row was
/// updated.
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

/// Resolve the `auto_fail` preference a `path` inherits from its Projet, for the
/// `project` tier of [`crate::auto_fail::resolve_auto_fail`]. `None` ⇒ the path
/// is in no Projet, or its Projet states no preference — the tier is transparent
/// either way.
pub(crate) async fn auto_fail_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<bool>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.auto_fail))
}

/// The Projet that currently owns `path`, if any. Read-then-decide basis for the
/// named refusal, and re-usable as a plain lookup. Compared **verbatim**.
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

/// Attach a member path to a Projet, enforcing at-most-one membership **before
/// any write**. A path already owned by a *different* Projet is refused, naming
/// the owner; a re-attach to the same Projet is an idempotent no-op.
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

/// Detach a member path from **any** Projet (the path is globally unique). Returns
/// `true` iff a membership row was removed.
pub(crate) async fn remove_member(db: &SqlitePool, path: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM project_members WHERE path = ?")
        .bind(path)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Delete a Projet and all its memberships. Returns `true` iff a Projet row was
/// removed. Used when a group is un-named back to its derived label.
pub(crate) async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM project_members WHERE project_id = ?")
        .bind(id)
        .execute(db)
        .await?;
    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Resolve the harness a `path` inherits from its Projet, for the `project` tier
/// of [`crate::harness_resolver`]. `None` ⇒ the path is in no Projet, or its
/// Projet carries no harness — the tier is transparent either way. An empty
/// stored harness is already normalised to `None` by [`owner_of`] → [`get`].
pub(crate) async fn harness_for_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<String>, sqlx::Error> {
    Ok(owner_of(db, path).await?.and_then(|p| p.harness))
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
        // The "migration de table idempotente" AC: running init twice on the same
        // pool is a no-op, never an error (mirror of trigger_store's guarantee).
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        // And a project created before the second init survives it.
        let p = create(&db, "P").await.unwrap();
        init(&db).await.unwrap();
        assert!(get(&db, &p.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn no_row_exists_before_naming() {
        // AC: aucune ligne de Projet tant qu'un humain n'a pas nommé un groupe.
        // A fresh store has zero rows and no path is owned.
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
        // AC: un chemin déjà membre ne peut pas être ajouté à un second — refus
        // nommant le propriétaire, avant tout effet.
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        let b = create(&db, "Bravo").await.unwrap();

        assert_eq!(
            add_member(&db, &a.id, "/repos/front").await.unwrap(),
            AddMember::Added
        );
        // Second attach to a DIFFERENT project: refused, naming Alpha.
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
        // The path still belongs to Alpha only.
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
        // reconciles them. A trailing slash makes a distinct, unowned key.
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();
        assert!(owner_of(&db, "/repos/front").await.unwrap().is_some());
        assert!(owner_of(&db, "/repos/front/").await.unwrap().is_none());
        assert!(owner_of(&db, "/repos/FRONT").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn harness_for_path_reads_the_owning_projects_harness() {
        // The `project` tier the spawn seam reads: a member path inherits its
        // Projet's harness; a blank harness collapses to None (#347).
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
        // Both members inherit it — a Projet setting is posed once for its repos.
        assert_eq!(
            harness_for_path(&db, "/repos/back")
                .await
                .unwrap()
                .as_deref(),
            Some("opencode")
        );
        // A non-member path inherits nothing.
        assert!(harness_for_path(&db, "/repos/other")
            .await
            .unwrap()
            .is_none());
        // Clearing the harness (empty string → NULL) makes the tier transparent.
        assert!(set_harness(&db, &a.id, Some("")).await.unwrap());
        assert!(harness_for_path(&db, "/repos/front")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn rename_and_delete() {
        let db = mem_db().await;
        let a = create(&db, "Alpha").await.unwrap();
        add_member(&db, &a.id, "/repos/front").await.unwrap();

        assert!(rename(&db, &a.id, "Renamed").await.unwrap());
        assert_eq!(get(&db, &a.id).await.unwrap().unwrap().name, "Renamed");

        // Deleting a Projet frees its member paths for another Projet.
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
}
