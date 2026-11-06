//! Persistence for **Espaces** (#1041, CONTEXT.md « Espace », ADR-0083) — a named,
//! coloured grouping of Runs, chosen by the user to sort the work of a shared
//! instance (typically one Espace per person).
//!
//! **Outside the event log, on purpose** (ADR-0083). Filing a Run in an Espace is
//! a display preference of the instance, not a fact of the Run's execution: it is
//! never replayed with the Run, and moving a Run never lengthens its history. It
//! also keeps a downgrade safe — an older binary reads any unknown event kind as
//! `RunStarted`, whereas it simply ignores a table it does not know.
//!
//! Two tables:
//! - `spaces` — one row per Espace: a name and a colour **key** from the fixed
//!   [`PALETTE`] (the front resolves the key to a per-theme value).
//! - `run_spaces` — the membership `run_id → space_id`. `run_id` is the PRIMARY
//!   KEY, so "a Run belongs to at most one Espace" is a schema invariant. No row
//!   means « Sans espace ». **Nothing is stored for a child Run**: it follows its
//!   root ([`effective_space`]).
//!
//! An Espace carries **no configuration** (no harness, no Pipelines, no skills):
//! it sorts, it never resolves anything.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// The fixed palette of an Espace's colour (#1041): eight keys, in the order a
/// new Espace takes them ([`first_free_color`]). Only the KEY is stored; the
/// front maps each to a value per theme, chosen so the eight stay apart under
/// red-green colour blindness (Okabe–Ito hues) and readable on both themes.
pub(crate) const PALETTE: [&str; 8] = [
    "blue",
    "orange",
    "green",
    "pink",
    "sky",
    "vermillion",
    "violet",
    "yellow",
];

/// A persisted Espace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Space {
    pub id: String,
    pub name: String,
    /// A key of [`PALETTE`].
    pub color: String,
}

pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS spaces (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            color      TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // `run_id` is the PRIMARY KEY: at-most-one-Espace-per-Run is a schema
    // invariant. No FK cascade — `delete` clears memberships explicitly (the
    // daemon never enables `PRAGMA foreign_keys`), same posture as
    // `project_members`.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS run_spaces (
            run_id      TEXT PRIMARY KEY,
            space_id    TEXT NOT NULL,
            assigned_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_run_spaces_space ON run_spaces(space_id)")
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) fn generate_space_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("spc-{ts}-{short}")
}

/// Whether `color` is a key of the fixed palette.
pub(crate) fn is_palette_color(color: &str) -> bool {
    PALETTE.contains(&color)
}

/// The colour a new Espace takes: the first palette key no Espace uses yet. Once
/// all eight are taken, the first among the least used, so colours stay as spread
/// out as eight allow.
pub(crate) fn first_free_color(used: &[String]) -> &'static str {
    let count = |c: &str| used.iter().filter(|u| u.as_str() == c).count();
    let min = PALETTE.iter().map(|c| count(c)).min().unwrap_or(0);
    PALETTE
        .iter()
        .copied()
        .find(|c| count(c) == min)
        .unwrap_or(PALETTE[0])
}

fn row_to_space(row: &sqlx::sqlite::SqliteRow) -> Space {
    Space {
        id: row.get("id"),
        name: row.get("name"),
        color: row.get("color"),
    }
}

/// Every Espace, in creation order (the list sorts them for display).
pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<Space>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name, color FROM spaces ORDER BY created_at ASC, rowid ASC")
        .fetch_all(db)
        .await?;
    Ok(rows.iter().map(row_to_space).collect())
}

pub(crate) async fn get(db: &SqlitePool, id: &str) -> Result<Option<Space>, sqlx::Error> {
    let row = sqlx::query("SELECT id, name, color FROM spaces WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.as_ref().map(row_to_space))
}

/// Create an Espace. `color: None` takes the first free colour of the palette;
/// the caller has already checked an explicit one against [`PALETTE`].
pub(crate) async fn create(
    db: &SqlitePool,
    name: &str,
    color: Option<&str>,
) -> Result<Space, sqlx::Error> {
    let color = match color {
        Some(c) => c.to_string(),
        None => {
            let used: Vec<String> = list(db).await?.into_iter().map(|s| s.color).collect();
            first_free_color(&used).to_string()
        }
    };
    let id = generate_space_id();
    sqlx::query("INSERT INTO spaces (id, name, color, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(&color)
        .bind(crate::event_log::now_iso())
        .execute(db)
        .await?;
    Ok(Space {
        id,
        name: name.to_string(),
        color,
    })
}

/// Rename and/or recolour. `false` when no such Espace.
pub(crate) async fn update(
    db: &SqlitePool,
    id: &str,
    name: Option<&str>,
    color: Option<&str>,
) -> Result<bool, sqlx::Error> {
    if get(db, id).await?.is_none() {
        return Ok(false);
    }
    if let Some(name) = name {
        sqlx::query("UPDATE spaces SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(db)
            .await?;
    }
    if let Some(color) = color {
        sqlx::query("UPDATE spaces SET color = ? WHERE id = ?")
            .bind(color)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(true)
}

/// Delete an Espace. Its Runs go back to « Sans espace » (their memberships are
/// dropped, the Runs themselves are untouched) and a Trigger that carried it
/// fires into « Sans espace » from now on. One transaction: nobody can observe a
/// membership pointing at a deleted Espace.
pub(crate) async fn delete(db: &SqlitePool, id: &str) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM run_spaces WHERE space_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE triggers SET space_id = NULL WHERE space_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("DELETE FROM spaces WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

/// File `run_id` in `space_id`, or back in « Sans espace » with `None`. The
/// caller has checked the Espace exists. Moving replaces the row — a Run is in at
/// most one Espace.
pub(crate) async fn assign(
    db: &SqlitePool,
    run_id: &str,
    space_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    match space_id {
        Some(space_id) => {
            sqlx::query(
                "INSERT INTO run_spaces (run_id, space_id, assigned_at) VALUES (?, ?, ?)
                 ON CONFLICT(run_id) DO UPDATE SET
                    space_id = excluded.space_id,
                    assigned_at = excluded.assigned_at",
            )
            .bind(run_id)
            .bind(space_id)
            .bind(crate::event_log::now_iso())
            .execute(db)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM run_spaces WHERE run_id = ?")
                .bind(run_id)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

/// The Espace stored for `run_id` itself (no inheritance — see
/// [`effective_space`]). Production reads go through [`memberships`].
#[cfg(test)]
pub(crate) async fn space_of(db: &SqlitePool, run_id: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT space_id FROM run_spaces WHERE run_id = ?")
        .bind(run_id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| r.get("space_id")))
}

/// Every stored membership, `run_id → space_id` — one read for the whole list.
pub(crate) async fn memberships(db: &SqlitePool) -> Result<HashMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT run_id, space_id FROM run_spaces")
        .fetch_all(db)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("run_id"), r.get("space_id")))
        .collect())
}

/// Forgetting a Run drops its membership, inside the forget transaction.
pub(crate) async fn forget_run(
    conn: &mut sqlx::SqliteConnection,
    run_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM run_spaces WHERE run_id = ?")
        .bind(run_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// The Espace a Run is listed in: its own row, else the nearest ancestor's — a
/// child Run follows its root (CONTEXT.md « Espace »), and nothing is stored for
/// it. `parent_of` maps a Run to its parent (`None` for a root); the walk stops
/// at the first stored row, at a Run with no known parent, or after a bounded
/// number of hops (a corrupted cycle must not spin).
pub(crate) fn effective_space(
    run_id: &str,
    parent_of: &HashMap<String, Option<String>>,
    memberships: &HashMap<String, String>,
) -> Option<String> {
    let mut current = run_id.to_string();
    for _ in 0..64 {
        if let Some(space) = memberships.get(&current) {
            return Some(space.clone());
        }
        match parent_of.get(&current).cloned().flatten() {
            Some(parent) if parent != current => current = parent,
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // `delete` also clears a Trigger's Espace.
        crate::trigger_store::init(&db).await.unwrap();
        init(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn init_is_idempotent_and_keeps_existing_rows() {
        let db = mem_db().await;
        let s = create(&db, "Loïc", None).await.unwrap();
        assign(&db, "run-1", Some(&s.id)).await.unwrap();
        init(&db).await.unwrap();
        assert_eq!(get(&db, &s.id).await.unwrap(), Some(s.clone()));
        assert_eq!(space_of(&db, "run-1").await.unwrap(), Some(s.id));
    }

    #[tokio::test]
    async fn a_database_from_before_spaces_gains_both_tables_empty() {
        // A pre-#1041 database: only the tables an older binary knew.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::trigger_store::init(&db).await.unwrap();
        for table in ["spaces", "run_spaces"] {
            let found = sqlx::query("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?")
                .bind(table)
                .fetch_optional(&db)
                .await
                .unwrap();
            assert!(found.is_none(), "{table} must not pre-exist");
        }
        init(&db).await.unwrap();
        assert!(list(&db).await.unwrap().is_empty());
        assert!(memberships(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn crud_round_trips_name_and_colour() {
        let db = mem_db().await;
        let s = create(&db, "Houss", Some("violet")).await.unwrap();
        assert_eq!(s.color, "violet");
        assert_eq!(list(&db).await.unwrap(), vec![s.clone()]);

        assert!(update(&db, &s.id, Some("Houssam"), Some("sky"))
            .await
            .unwrap());
        let got = get(&db, &s.id).await.unwrap().unwrap();
        assert_eq!((got.name.as_str(), got.color.as_str()), ("Houssam", "sky"));
        assert!(!update(&db, "spc-nope", Some("x"), None).await.unwrap());

        assert!(delete(&db, &s.id).await.unwrap());
        assert!(get(&db, &s.id).await.unwrap().is_none());
        assert!(!delete(&db, &s.id).await.unwrap());
    }

    #[test]
    fn a_new_space_takes_the_first_free_colour() {
        assert_eq!(first_free_color(&[]), "blue");
        assert_eq!(first_free_color(&["blue".into()]), "orange");
        // A gap left by a deleted or recoloured Espace is filled first.
        assert_eq!(first_free_color(&["blue".into(), "green".into()]), "orange");
        // All eight taken: the first of the least used.
        let mut all: Vec<String> = PALETTE.iter().map(|c| c.to_string()).collect();
        assert_eq!(first_free_color(&all), "blue");
        all.push("blue".into());
        assert_eq!(first_free_color(&all), "orange");
    }

    #[tokio::test]
    async fn create_without_colour_picks_the_first_free_one() {
        let db = mem_db().await;
        let a = create(&db, "A", None).await.unwrap();
        let b = create(&db, "B", None).await.unwrap();
        assert_eq!((a.color.as_str(), b.color.as_str()), ("blue", "orange"));
        update(&db, &a.id, None, Some("yellow")).await.unwrap();
        assert_eq!(create(&db, "C", None).await.unwrap().color, "blue");
    }

    #[tokio::test]
    async fn a_run_is_in_at_most_one_space_and_moves() {
        let db = mem_db().await;
        let a = create(&db, "A", None).await.unwrap();
        let b = create(&db, "B", None).await.unwrap();
        assign(&db, "run-1", Some(&a.id)).await.unwrap();
        assign(&db, "run-1", Some(&b.id)).await.unwrap();
        assert_eq!(space_of(&db, "run-1").await.unwrap(), Some(b.id.clone()));
        assert_eq!(memberships(&db).await.unwrap().len(), 1);
        // Back to « Sans espace ».
        assign(&db, "run-1", None).await.unwrap();
        assert_eq!(space_of(&db, "run-1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn deleting_a_space_sends_its_runs_and_triggers_back_to_no_space() {
        let db = mem_db().await;
        let a = create(&db, "A", None).await.unwrap();
        let b = create(&db, "B", None).await.unwrap();
        assign(&db, "run-a", Some(&a.id)).await.unwrap();
        assign(&db, "run-b", Some(&b.id)).await.unwrap();
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, pipeline_name, input_template, variables, \
             cron, overlap_policy, enabled, created_at, space_id) \
             VALUES ('trg-1', 'T', 'p', 'p', '', '{}', '* * * * *', 'skip', 1, 'now', ?)",
        )
        .bind(&a.id)
        .execute(&db)
        .await
        .unwrap();

        assert!(delete(&db, &a.id).await.unwrap());
        assert_eq!(space_of(&db, "run-a").await.unwrap(), None);
        // Another Espace's Runs are untouched.
        assert_eq!(space_of(&db, "run-b").await.unwrap(), Some(b.id));
        let trigger = crate::trigger_store::get(&db, "trg-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(trigger.space_id, None);
    }

    #[tokio::test]
    async fn forgetting_a_run_drops_its_membership() {
        let db = mem_db().await;
        let a = create(&db, "A", None).await.unwrap();
        assign(&db, "run-1", Some(&a.id)).await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        forget_run(&mut conn, "run-1").await.unwrap();
        drop(conn);
        assert!(memberships(&db).await.unwrap().is_empty());
    }

    #[test]
    fn a_child_follows_its_root() {
        let parents: HashMap<String, Option<String>> = [
            ("root".to_string(), None),
            ("child".to_string(), Some("root".to_string())),
            ("grandchild".to_string(), Some("child".to_string())),
            ("loner".to_string(), None),
        ]
        .into_iter()
        .collect();
        let rows: HashMap<String, String> = [("root".to_string(), "spc-a".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            effective_space("grandchild", &parents, &rows).as_deref(),
            Some("spc-a")
        );
        assert_eq!(
            effective_space("child", &parents, &rows).as_deref(),
            Some("spc-a")
        );
        assert_eq!(effective_space("loner", &parents, &rows), None);
        // A parent that is not listed (forgotten) ends the walk.
        let orphan: HashMap<String, Option<String>> = [("o".to_string(), Some("gone".to_string()))]
            .into_iter()
            .collect();
        assert_eq!(effective_space("o", &orphan, &rows), None);
        // A corrupted cycle terminates.
        let cycle: HashMap<String, Option<String>> = [
            ("x".to_string(), Some("y".to_string())),
            ("y".to_string(), Some("x".to_string())),
        ]
        .into_iter()
        .collect();
        assert_eq!(effective_space("x", &cycle, &rows), None);
    }
}
