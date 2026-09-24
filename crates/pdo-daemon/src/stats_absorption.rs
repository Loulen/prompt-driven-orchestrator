//! **Absorption** in Stats (#888, ADR-0077): one Stats row — the **absorbent** —
//! counts the data of other rows of the same level, its **absorbed** members, in
//! every tab that has that dimension. "Combine" in the UI.
//!
//! Three pieces, one module:
//!
//! - **The store** — a table of `pdo.db`, instance configuration beside
//!   `instance_config` (ADR-0015). One row per member: `(dimension, scope,
//!   member)` is unique, so a member has exactly one absorbent. The storage is
//!   **flat, one level**: [`combine`] flattens on write (absorbing an absorbent
//!   moves its members over), so the read never resolves anything transitively.
//! - **The resolver** — [`AbsorptionResolver`], loaded once per Stats request
//!   and applied where each fold reads a Run's identity, BEFORE aggregating. The
//!   event log is never rewritten: `uncombined=true` is just a read with an empty
//!   resolver, and removing a member takes effect at once, history included.
//! - **The Run identity read** — [`RunIdentity`]: the `pipeline_id` →
//!   `pipeline_name` → `(unknown)` fallback and the frozen `node_defs`, read in
//!   ONE place instead of being copied into Sessions, Cost and Performance. It is
//!   where the resolver substitutes an absorbed key with its absorbent's.
//!
//! #890 ships the `pipeline` dimension; #891 adds the rename-born absorption
//! ([`record_rename`], origin `rename`); #892 adds the two other dimensions:
//!
//! - **Nodes**, scoped to the key of their absorbent Pipeline (or of a lone
//!   Pipeline). A Node absorption applies to every Run counted under that
//!   Pipeline row — in every tree where the Node shows. Taking a Pipeline out
//!   of its absorption takes its Nodes out of the Node absorptions of that
//!   scope ([`remove_member`]).
//! - **Models**, by verbatim id (ADR-0065): they fold the « By model » axis
//!   of Cost and Performance and, since #906 (ADR-0078 amending ADR-0077), the
//!   model × effort couples of every Node too; their couples meet by effort.
//!
//! #906 adds two more (ADR-0078), both with a **frozen scope**: an absorption
//! is written once per RAW key it covered when it was posed, and read on the
//! raw key — no carry, no cascade:
//!
//! - **Efforts** of one model, scoped to a raw model id. Posed on a model row,
//!   it is written for the row's key and for every model it absorbed then.
//! - **Couples** (model × effort) of one Node, scoped to a raw `(Pipeline,
//!   Node)` pair ([`couple_scope`]). Local to the Node row: never on the
//!   « By model » axis. Its keys are couples as the global absorptions resolve
//!   them, and they go through that resolution again at read time.
//!
//! A couple of a Node resolves in this order ([`RunIdentity::couple`]): (1) the
//! effort, on the raw model; (2) the model; (3) the couples meet by effort;
//! (4) the couple, on the raw Node. The « By model » axis stops at step 3.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::AppState;

/// The three dimensions an absorption can have.
pub(crate) const PIPELINE: &str = "pipeline";
pub(crate) const NODE: &str = "node";
pub(crate) const MODEL: &str = "model";
pub(crate) const EFFORT: &str = "effort";
pub(crate) const COUPLE: &str = "couple";

/// The key of an effort: the effort itself, `""` for « not set ».
pub(crate) fn effort_key(effort: Option<&str>) -> String {
    effort.unwrap_or("").to_string()
}

fn effort_of_key(key: &str) -> Option<String> {
    (!key.is_empty()).then(|| key.to_string())
}

/// The name of an effort, as its row shows it.
pub(crate) fn effort_name(effort: Option<&str>) -> String {
    effort.unwrap_or("not set").to_string()
}

/// The key of a model × effort couple: `model|effort` (`model|` for « not
/// set »). A model id never carries a `|`.
pub(crate) fn couple_key(model: &str, effort: Option<&str>) -> String {
    format!("{model}|{}", effort.unwrap_or(""))
}

/// `model|effort` → `(model, effort)`; `None` on a key that is no couple.
pub(crate) fn split_couple_key(key: &str) -> Option<(String, Option<String>)> {
    let (model, effort) = key.rsplit_once('|')?;
    (!model.is_empty()).then(|| (model.to_string(), effort_of_key(effort)))
}

/// The name of a couple, as its row shows it: `model · effort`.
pub(crate) fn couple_name(model: &str, effort: Option<&str>) -> String {
    format!("{model} · {}", effort_name(effort))
}

/// The scope of a couple absorption: one `(Pipeline, Node)` pair, as a JSON
/// array — unambiguous whatever the two keys hold.
pub(crate) fn couple_scope(pipeline: &str, node: &str) -> String {
    serde_json::json!([pipeline, node]).to_string()
}

fn parse_couple_scope(scope: &str) -> Option<(String, String)> {
    let pair: (String, String) = serde_json::from_str(scope).ok()?;
    (!pair.0.is_empty() && !pair.1.is_empty()).then_some(pair)
}

/// Where an absorption came from: the operator's Combine (`manual`) or a
/// Pipeline rename that changed its id (`rename`, #891).
pub(crate) const ORIGIN_MANUAL: &str = "manual";
pub(crate) const ORIGIN_RENAME: &str = "rename";

/// The Pipeline key of a Run whose `run_started` names neither an id nor a name.
pub(crate) const UNKNOWN_PIPELINE: &str = "(unknown)";

pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
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

/// One stored member row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct AbsorptionRow {
    pub dimension: String,
    pub scope: String,
    pub scope_name: String,
    pub member: String,
    pub member_name: String,
    pub absorbent: String,
    pub absorbent_name: String,
    pub origin: String,
    pub created_at: String,
}

/// A key and the name to show for it. The UI only ever shows the name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Named {
    pub key: String,
    pub name: String,
}

pub(crate) async fn list(db: &SqlitePool) -> Result<Vec<AbsorptionRow>, sqlx::Error> {
    sqlx::query_as::<_, AbsorptionRow>(
        "SELECT dimension, scope, scope_name, member, member_name, absorbent, absorbent_name, \
                origin, created_at \
         FROM stats_absorptions ORDER BY dimension, scope, absorbent, created_at, member",
    )
    .fetch_all(db)
    .await
}

/// Where an absorption lives: its dimension, and — for a Node absorption — the
/// key and name of the Pipeline row it is scoped to (empty otherwise).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scope<'a> {
    pub dimension: &'a str,
    pub key: &'a str,
    pub name: &'a str,
}

/// Create or extend an absorption, flattened on write (ADR-0077 §3):
///
/// 1. the absorbent leaves whatever absorbed it — the operator just asked it to
///    carry the series, so it is a top-level row from now on;
/// 2. every member that was itself an absorbent hands its members over;
/// 3. every member now points at the absorbent (a member already absorbed
///    elsewhere moves: one absorbent per member);
/// 4. the absorbent's name is refreshed on all its rows — it is the name shown
///    when the absorbent has no Run in the period.
///
/// A chain A → B → C therefore always ends as C absorbing A and B.
///
/// A Pipeline Combine also carries the members' Node absorptions over to the
/// absorbent's scope (#892): a Node absorption is scoped to the Pipeline row
/// that shows the Node, and the members' rows are now the absorbent's.
pub(crate) async fn combine(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    combine_scoped(
        db,
        Scope {
            dimension,
            key: scope,
            name: "",
        },
        absorbent,
        members,
        origin,
    )
    .await
}

/// [`combine`], with the scope's display name (a Node absorption's Pipeline).
pub(crate) async fn combine_scoped(
    db: &SqlitePool,
    scope: Scope<'_>,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    combine_in(&mut tx, scope, absorbent, members, origin).await?;
    if scope.dimension == PIPELINE {
        for member in members.iter().filter(|m| m.key != absorbent.key) {
            carry_node_absorptions(&mut tx, &member.key, absorbent).await?;
        }
    }
    tx.commit().await
}

async fn combine_in(
    tx: &mut sqlx::SqliteConnection,
    scope: Scope<'_>,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    let now = crate::event_log::now_iso();
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

/// Pose an effort or couple absorption with its **frozen scope** (ADR-0078):
/// one write per raw key the row covers right now — its own key and the keys
/// it absorbs at this moment — each flattened like any Combine. Nothing is
/// carried later: a row absorbed afterwards keeps its own rows, a member
/// absorbed afterwards is not covered, a member taken out keeps its rows.
///
/// - `effort`: `scope` is the model row's key; the raw scopes are that id and
///   the model ids it absorbs.
/// - `couple`: `scope` is [`couple_scope`] of the Node row's Pipeline and Node
///   keys; the raw scopes are every raw `(Pipeline, Node)` pair counted there —
///   the row's Pipeline and the Pipelines it absorbs, crossed with the row's
///   Node and the Nodes it absorbs, kept where the Node really ran.
pub(crate) async fn combine_frozen(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    scope_name: &str,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    let covered = covered_scopes(&mut tx, dimension, scope, scope_name).await?;
    for raw in &covered {
        combine_in(
            &mut tx,
            Scope {
                dimension,
                key: &raw.key,
                name: &raw.name,
            },
            absorbent,
            members,
            origin,
        )
        .await?;
    }
    tx.commit().await
}

async fn members_of(
    tx: &mut sqlx::SqliteConnection,
    dimension: &str,
    scope: &str,
    absorbent: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT member FROM stats_absorptions \
         WHERE dimension = ? AND scope = ? AND absorbent = ? ORDER BY member",
    )
    .bind(dimension)
    .bind(scope)
    .bind(absorbent)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rows.into_iter().map(|(key,)| key).collect())
}

/// The raw scopes an effort or couple absorption posed on `scope` covers now.
async fn covered_scopes(
    tx: &mut sqlx::SqliteConnection,
    dimension: &str,
    scope: &str,
    scope_name: &str,
) -> Result<Vec<Named>, sqlx::Error> {
    if dimension == EFFORT {
        let mut covered = vec![Named {
            key: scope.to_string(),
            name: scope.to_string(),
        }];
        for model in members_of(tx, MODEL, "", scope).await? {
            covered.push(Named {
                key: model.clone(),
                name: model,
            });
        }
        return Ok(covered);
    }
    let Some((pipeline, node)) = parse_couple_scope(scope) else {
        return Ok(Vec::new());
    };
    let mut pipelines = vec![pipeline.clone()];
    pipelines.extend(members_of(tx, PIPELINE, "", &pipeline).await?);
    let mut nodes = vec![node.clone()];
    nodes.extend(members_of(tx, NODE, &pipeline, &node).await?);
    let mut covered = Vec::new();
    for raw_pipeline in &pipelines {
        let ran = nodes_that_ran_under(tx, raw_pipeline).await?;
        for raw_node in &nodes {
            let own = raw_pipeline == &pipeline && raw_node == &node;
            if !own && !ran.contains(raw_node) {
                continue;
            }
            let name = if own && !scope_name.is_empty() {
                scope_name.to_string()
            } else {
                node_label(tx, raw_pipeline, raw_node).await?
            };
            covered.push(Named {
                key: couple_scope(raw_pipeline, raw_node),
                name,
            });
        }
    }
    Ok(covered)
}

/// « Node (Pipeline) », by the names the most recent Run of `pipeline` froze.
async fn node_label(
    tx: &mut sqlx::SqliteConnection,
    pipeline: &str,
    node: &str,
) -> Result<String, sqlx::Error> {
    let payload: Option<(String,)> = sqlx::query_as(
        "SELECT payload FROM events WHERE kind = 'run_started' \
           AND COALESCE(json_extract(payload, '$.pipeline_id'), \
                        json_extract(payload, '$.pipeline_name'), ?) = ? \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(UNKNOWN_PIPELINE)
    .bind(pipeline)
    .fetch_optional(&mut *tx)
    .await?;
    let payload: Value = payload
        .and_then(|(text,)| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let (_, pipeline_name) = if payload.is_null() {
        (pipeline.to_string(), pipeline.to_string())
    } else {
        pipeline_identity(&payload)
    };
    let node_name = node_defs(&payload)
        .get(node)
        .map(|def| def.name.clone())
        .unwrap_or_else(|| node.to_string());
    Ok(format!("{node_name} ({pipeline_name})"))
}

async fn rows_of_scope(
    tx: &mut sqlx::SqliteConnection,
    dimension: &str,
    scope: &str,
) -> Result<Vec<AbsorptionRow>, sqlx::Error> {
    sqlx::query_as::<_, AbsorptionRow>(
        "SELECT dimension, scope, scope_name, member, member_name, absorbent, absorbent_name, \
                origin, created_at \
         FROM stats_absorptions WHERE dimension = ? AND scope = ? \
         ORDER BY absorbent, created_at, member",
    )
    .bind(dimension)
    .bind(scope)
    .fetch_all(&mut *tx)
    .await
}

/// Move the Node absorptions scoped to Pipeline `from` under `to`, which just
/// absorbed it. What `to`'s scope already says wins: a Node it already
/// absorbs stays where it is, and a moved absorbent that `to` already absorbs
/// hands over to that absorbent — one level, always.
async fn carry_node_absorptions(
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

/// A library rename that changed a Pipeline's id (ADR-0077 §4): the new key
/// carries the old key's series, as a Combine would, origin `rename`. Flattened
/// like any Combine — a chain A → B → C ends as C absorbing A and B. When the
/// old key was itself absorbed, the new key joins that same absorbent instead:
/// the operator's reading is kept, and there is still only one level.
pub(crate) async fn record_rename(
    db: &SqlitePool,
    old: &Named,
    new: &Named,
) -> Result<(), sqlx::Error> {
    if old.key == new.key {
        return Ok(());
    }
    let absorbent: Option<(String, String)> = sqlx::query_as(
        "SELECT absorbent, absorbent_name FROM stats_absorptions \
         WHERE dimension = ? AND scope = '' AND member = ?",
    )
    .bind(PIPELINE)
    .bind(&old.key)
    .fetch_optional(db)
    .await?;
    match absorbent {
        Some((key, name)) if key != new.key => {
            combine(
                db,
                PIPELINE,
                "",
                &Named { key, name },
                std::slice::from_ref(new),
                ORIGIN_RENAME,
            )
            .await
        }
        _ => {
            combine(
                db,
                PIPELINE,
                "",
                new,
                std::slice::from_ref(old),
                ORIGIN_RENAME,
            )
            .await
        }
    }
}

/// [`record_rename`] for the rename handlers: the files have already moved, so
/// a failed write here is logged, never turned into a failed rename.
pub(crate) async fn record_rename_best_effort(db: &SqlitePool, old: Named, new: Named) {
    if let Err(e) = record_rename(db, &old, &new).await {
        tracing::warn!(
            "stats absorption for the rename {} -> {} failed: {e}",
            old.key,
            new.key
        );
    }
}

/// Take one member out of its absorption. Removing the last member leaves no
/// row for that absorbent — the absorption is gone, and its rows come back.
/// `false` when the key was not absorbed.
///
/// Taking a Pipeline out also takes its Nodes out of the Node absorptions of
/// its former absorbent's scope (#892, grilling Q13): a Node that ran under
/// the removed Pipeline and under none of the Pipelines still counted in that
/// row leaves, and so does every member of a Node absorbent in that case —
/// they find their own rows again. Which Pipeline a Node ran under is read off
/// the event log, never guessed.
pub(crate) async fn remove_member(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    member: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    let absorbent: Option<(String,)> = sqlx::query_as(
        "SELECT absorbent FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?",
    )
    .bind(dimension)
    .bind(scope)
    .bind(member)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((absorbent,)) = absorbent else {
        return Ok(false);
    };
    sqlx::query("DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?")
        .bind(dimension)
        .bind(scope)
        .bind(member)
        .execute(&mut *tx)
        .await?;
    if dimension == PIPELINE {
        release_nodes_of(&mut tx, member, &absorbent).await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// The Node ids that ran under the raw Pipeline key `pipeline`, whole history.
async fn nodes_that_ran_under(
    tx: &mut sqlx::SqliteConnection,
    pipeline: &str,
) -> Result<BTreeSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT ns.node_id FROM events ns \
         JOIN events rs ON rs.run_id = ns.run_id AND rs.kind = 'run_started' \
         WHERE ns.kind = 'node_started' AND ns.node_id IS NOT NULL \
           AND COALESCE(json_extract(rs.payload, '$.pipeline_id'), \
                        json_extract(rs.payload, '$.pipeline_name'), ?) = ?",
    )
    .bind(UNKNOWN_PIPELINE)
    .bind(pipeline)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// The cascade of [`remove_member`] for a Pipeline `removed` that left
/// `absorbent`'s row.
async fn release_nodes_of(
    tx: &mut sqlx::SqliteConnection,
    removed: &str,
    absorbent: &str,
) -> Result<(), sqlx::Error> {
    let rows = rows_of_scope(tx, NODE, absorbent).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let gone = nodes_that_ran_under(tx, removed).await?;
    let mut staying = nodes_that_ran_under(tx, absorbent).await?;
    let remaining: Vec<(String,)> = sqlx::query_as(
        "SELECT member FROM stats_absorptions WHERE dimension = ? AND scope = '' AND absorbent = ?",
    )
    .bind(PIPELINE)
    .bind(absorbent)
    .fetch_all(&mut *tx)
    .await?;
    for (pipeline,) in remaining {
        staying.extend(nodes_that_ran_under(tx, &pipeline).await?);
    }
    let leaves = |node: &str| gone.contains(node) && !staying.contains(node);
    for row in rows {
        if leaves(&row.member) || leaves(&row.absorbent) {
            sqlx::query(
                "DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?",
            )
            .bind(NODE)
            .bind(absorbent)
            .bind(&row.member)
            .execute(&mut *tx)
            .await?;
        }
    }
    Ok(())
}

// --- The resolver ---------------------------------------------------------------

/// One dimension's absorptions within one scope: member → absorbent, the name
/// stored for each absorbent at its last Combine, and each absorbent's members.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopeAbsorptions {
    absorbent_of: HashMap<String, String>,
    absorbent_names: HashMap<String, String>,
    members: BTreeMap<String, Vec<Named>>,
}

impl ScopeAbsorptions {
    fn insert(&mut self, row: &AbsorptionRow) {
        self.absorbent_of
            .insert(row.member.clone(), row.absorbent.clone());
        self.absorbent_names
            .insert(row.absorbent.clone(), row.absorbent_name.clone());
        self.members
            .entry(row.absorbent.clone())
            .or_default()
            .push(Named {
                key: row.member.clone(),
                name: row.member_name.clone(),
            });
    }

    /// The key a row of `key` is counted under.
    pub(crate) fn resolve<'a>(&'a self, key: &'a str) -> &'a str {
        self.absorbent_of
            .get(key)
            .map(String::as_str)
            .unwrap_or(key)
    }

    fn stored_name(&self, key: &str) -> Option<&str> {
        self.absorbent_names.get(key).map(String::as_str)
    }

    fn members_of(&self, absorbent: &str) -> &[Named] {
        self.members
            .get(absorbent)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The members of `absorbent` as its row lists them, with what each did in
    /// the period — every member, including one with no Run in it.
    fn absorbed(&self, absorbent: &str, tallies: &PipelineTallies) -> Vec<AbsorbedMember> {
        self.members
            .get(absorbent)
            .into_iter()
            .flatten()
            .map(|member| tallies.member(member))
            .collect()
    }
}

/// The couple absorptions of one raw `(Pipeline, Node)` pair (#906), their
/// keys resolved through the global absorptions — step 4 runs on couples the
/// global steps already resolved, so a global absorption posed later carries
/// a local one along (ADR-0078 §2).
#[derive(Debug, Clone, Default)]
pub(crate) struct CoupleScope {
    /// Resolved member couple → resolved absorbent couple.
    to: HashMap<String, String>,
    /// Each stored member, with its resolved key and its resolved absorbent's.
    members: Vec<(Named, String, String)>,
}

/// The global absorptions a couple goes through (steps 1–2): the efforts of a
/// raw model, then the model.
#[derive(Debug, Clone, Default)]
struct GlobalAbsorptions {
    models: Arc<ScopeAbsorptions>,
    /// Raw model id → its effort absorptions.
    efforts: Arc<HashMap<String, ScopeAbsorptions>>,
}

impl GlobalAbsorptions {
    fn effort(&self, raw_model: &str, raw_effort: Option<&str>) -> Option<String> {
        let key = effort_key(raw_effort);
        let resolved = self
            .efforts
            .get(raw_model)
            .map(|scope| scope.resolve(&key).to_string())
            .unwrap_or(key);
        effort_of_key(&resolved)
    }

    fn couple(&self, raw_model: &str, raw_effort: Option<&str>) -> (String, Option<String>) {
        (
            self.models.resolve(raw_model).to_string(),
            self.effort(raw_model, raw_effort),
        )
    }

    /// A stored couple key, resolved as if it were a raw couple.
    fn couple_key(&self, key: &str) -> Option<String> {
        let (model, effort) = split_couple_key(key)?;
        let (model, effort) = self.couple(&model, effort.as_deref());
        Some(couple_key(&model, effort.as_deref()))
    }
}

/// The absorptions of one Stats read. Empty under `uncombined=true`, which is
/// exactly « the rows as the event log wrote them ».
#[derive(Debug, Clone, Default)]
pub(crate) struct AbsorptionResolver {
    pipelines: ScopeAbsorptions,
    /// Pipeline key (of the row the Node shows under) → its Node absorptions.
    nodes: HashMap<String, Arc<ScopeAbsorptions>>,
    global: GlobalAbsorptions,
    /// [`couple_scope`] of a raw `(Pipeline, Node)` → its couple absorptions.
    couples: Arc<HashMap<String, CoupleScope>>,
    fingerprint: u64,
}

impl AbsorptionResolver {
    pub(crate) fn from_rows(rows: &[AbsorptionRow]) -> Self {
        let mut resolver = Self::default();
        let mut hasher = DefaultHasher::new();
        let mut sorted: Vec<&AbsorptionRow> = rows.iter().collect();
        sorted.sort_by(|a, b| {
            (&a.dimension, &a.scope, &a.member).cmp(&(&b.dimension, &b.scope, &b.member))
        });
        let mut nodes: HashMap<String, ScopeAbsorptions> = HashMap::new();
        let mut models = ScopeAbsorptions::default();
        let mut efforts: HashMap<String, ScopeAbsorptions> = HashMap::new();
        let mut couple_rows: Vec<&AbsorptionRow> = Vec::new();
        for row in sorted {
            (
                &row.dimension,
                &row.scope,
                &row.member,
                &row.member_name,
                &row.absorbent,
                &row.absorbent_name,
            )
                .hash(&mut hasher);
            match row.dimension.as_str() {
                PIPELINE => resolver.pipelines.insert(row),
                NODE => nodes.entry(row.scope.clone()).or_default().insert(row),
                MODEL => models.insert(row),
                EFFORT => efforts.entry(row.scope.clone()).or_default().insert(row),
                COUPLE => couple_rows.push(row),
                _ => {}
            }
        }
        resolver.nodes = nodes
            .into_iter()
            .map(|(scope, absorptions)| (scope, Arc::new(absorptions)))
            .collect();
        resolver.global = GlobalAbsorptions {
            models: Arc::new(models),
            efforts: Arc::new(efforts),
        };
        let mut couples: HashMap<String, CoupleScope> = HashMap::new();
        for row in couple_rows {
            let (Some(member), Some(absorbent)) = (
                resolver.global.couple_key(&row.member),
                resolver.global.couple_key(&row.absorbent),
            ) else {
                continue;
            };
            let scope = couples.entry(row.scope.clone()).or_default();
            if member != absorbent {
                scope
                    .to
                    .entry(member.clone())
                    .or_insert_with(|| absorbent.clone());
            }
            scope.members.push((
                Named {
                    key: row.member.clone(),
                    name: row.member_name.clone(),
                },
                member,
                absorbent,
            ));
        }
        resolver.couples = Arc::new(couples);
        resolver.fingerprint = hasher.finish();
        resolver
    }

    /// The resolver of one request: every stored absorption, or none at all
    /// under `uncombined`.
    pub(crate) async fn load(db: &SqlitePool, uncombined: bool) -> Result<Self, sqlx::Error> {
        if uncombined {
            return Ok(Self::default());
        }
        Ok(Self::from_rows(&list(db).await?))
    }

    /// The key a Run of Pipeline `key` is counted under.
    pub(crate) fn pipeline<'a>(&'a self, key: &'a str) -> &'a str {
        self.pipelines.resolve(key)
    }

    /// The name stored for an absorbent key at its last Combine or rename — for
    /// a row that has no Run of its own to name it.
    pub(crate) fn stored_name(&self, key: &str) -> Option<&str> {
        self.pipelines.stored_name(key)
    }

    /// Folded into every cache that keeps an already-aggregated result (ADR-0077
    /// §2): a Combine or an Uncombine changes it.
    pub(crate) fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The Run identity of one `run_started` payload, absorptions applied.
    pub(crate) fn run_identity(&self, payload: &Value) -> RunIdentity {
        let (raw_key, raw_name) = pipeline_identity(payload);
        let key = self.pipeline(&raw_key).to_string();
        let own = key == raw_key;
        let name = if own {
            raw_name.clone()
        } else {
            self.pipelines
                .stored_name(&key)
                .map(str::to_string)
                .unwrap_or_else(|| key.clone())
        };
        RunIdentity {
            nodes: self.nodes.get(&key).cloned().unwrap_or_default(),
            global: self.global.clone(),
            couples: self.couples.clone(),
            pipeline_key: key,
            pipeline_name: name,
            own_name: own,
            raw_pipeline_key: raw_key,
            node_defs: node_defs(payload),
            run_id: String::new(),
            started_at: String::new(),
        }
    }

    /// The members of Pipeline `absorbent` as its row lists them.
    pub(crate) fn absorbed(
        &self,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.pipelines.absorbed(absorbent, tallies)
    }

    /// The members of Node `absorbent` under Pipeline row `pipeline`.
    pub(crate) fn absorbed_nodes(
        &self,
        pipeline: &str,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.nodes
            .get(pipeline)
            .map(|scope| scope.absorbed(absorbent, tallies))
            .unwrap_or_default()
    }

    /// The members of model `absorbent` on the « By model » axis.
    pub(crate) fn absorbed_models(
        &self,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.global.models.absorbed(absorbent, tallies)
    }

    /// The members of effort `absorbent` (its key, `""` for « not set ») under
    /// a model row that counts the raw model ids `raw_models` (#906). Each
    /// member lists the raw models whose absorption holds it — its ✕ takes it
    /// out of all of them. `tallies` are keyed by raw effort key.
    pub(crate) fn absorbed_efforts<'a>(
        &self,
        raw_models: impl IntoIterator<Item = &'a str>,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        let mut members: BTreeMap<String, AbsorbedMember> = BTreeMap::new();
        for raw_model in raw_models {
            let Some(scope) = self.global.efforts.get(raw_model) else {
                continue;
            };
            for member in scope.members_of(absorbent) {
                members
                    .entry(member.key.clone())
                    .or_insert_with(|| AbsorbedMember {
                        name: effort_name(effort_of_key(&member.key).as_deref()),
                        ..tallies.member(member)
                    })
                    .scopes
                    .push(raw_model.to_string());
            }
        }
        members.into_values().collect()
    }

    /// The members of couple `absorbent` of a Node row that counts the raw
    /// `(Pipeline, Node)` scopes `scopes` (#906), each with the scopes that hold
    /// it. `tallies` are keyed by globally resolved couple key.
    pub(crate) fn absorbed_couples<'a>(
        &self,
        scopes: impl IntoIterator<Item = &'a String>,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        let mut members: BTreeMap<String, AbsorbedMember> = BTreeMap::new();
        for scope_key in scopes {
            let Some(scope) = self.couples.get(scope_key) else {
                continue;
            };
            for (member, resolved, resolved_absorbent) in &scope.members {
                if resolved_absorbent != absorbent {
                    continue;
                }
                members
                    .entry(member.key.clone())
                    .or_insert_with(|| AbsorbedMember {
                        key: member.key.clone(),
                        name: member.name.clone(),
                        ..tallies.member(&Named {
                            key: resolved.clone(),
                            name: member.name.clone(),
                        })
                    })
                    .scopes
                    .push(scope_key.clone());
            }
        }
        members.into_values().collect()
    }
}

/// A model × effort couple of a Node as its row counts it (#906): the four
/// resolution steps applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoupleIdentity {
    /// The couple the row is keyed by, after step 4.
    pub model: String,
    pub effort: Option<String>,
    pub key: String,
    /// The couple after the global steps (1–3) — the local members' tallies.
    pub global_key: String,
    /// The couple the event log wrote — the global members' tallies.
    pub raw_key: String,
    /// Whether a global absorption (models, efforts) moved it.
    pub global: bool,
    /// The raw `(Pipeline, Node)` scope its couple absorptions live in; empty
    /// on a row that is no Node (Infrastructure, Unassigned).
    pub scope: String,
}

// --- The Run identity read (prefactored out of Sessions, Cost, Performance) ----

/// The Pipeline key and name of a Run as the event log froze them: the
/// `pipeline_id` going forward (#377), else the always-present `pipeline_name`,
/// else `(unknown)`.
pub(crate) fn pipeline_identity(payload: &Value) -> (String, String) {
    let key = payload
        .get("pipeline_id")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("pipeline_name").and_then(|v| v.as_str()))
        .unwrap_or(UNKNOWN_PIPELINE)
        .to_string();
    let name = payload
        .get("pipeline_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&key)
        .to_string();
    (key, name)
}

/// One node of the Run's frozen pipeline snapshot, as Sessions and Cost read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NodeDef {
    pub name: String,
    pub node_type: String,
}

fn node_defs(payload: &Value) -> BTreeMap<String, NodeDef> {
    payload
        .get("node_defs")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|def| {
            let id = def.get("id")?.as_str()?;
            Some((
                id.to_string(),
                NodeDef {
                    name: def
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string(),
                    node_type: def
                        .get("node_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                },
            ))
        })
        .collect()
}

/// A Run's identity for the Stats folds, absorptions applied.
#[derive(Debug, Clone)]
pub(crate) struct RunIdentity {
    /// The key the Run is counted under — its absorbent's when it is absorbed.
    pub pipeline_key: String,
    /// The name to show for that key: the Run's own name, or, for an absorbed
    /// Run, the absorbent's stored name.
    pub pipeline_name: String,
    /// Whether the name is the Run's own. A row's name is its most recent own
    /// Run's name; an absorbed Run only names the row when the absorbent has no
    /// Run of its own in the period.
    pub own_name: bool,
    /// The key the event log wrote, before substitution — the member tallies.
    pub raw_pipeline_key: String,
    pub node_defs: BTreeMap<String, NodeDef>,
    /// The Node absorptions of the Pipeline row this Run is counted under.
    nodes: Arc<ScopeAbsorptions>,
    /// The Model and effort absorptions (steps 1–2 of a couple).
    global: GlobalAbsorptions,
    /// The couple absorptions, by raw `(Pipeline, Node)` scope.
    couples: Arc<HashMap<String, CoupleScope>>,
    /// The Run itself, for the tallies of a fold that only sees the identity
    /// (Performance's « By model » axis) — empty unless [`Self::for_run`] set it.
    pub run_id: String,
    pub started_at: String,
}

/// A Node of a Run as a Stats row counts it, Node absorptions applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NodeIdentity {
    /// The Node id the row is keyed by — its absorbent's when it is absorbed.
    pub key: String,
    /// The Node id the event log wrote — the member tallies.
    pub raw_key: String,
    pub name: String,
    /// Same naming rule as a Pipeline row: an absorbed Node only names the row
    /// when the absorbent Node has no execution of its own.
    pub own_name: bool,
}

impl NodeIdentity {
    /// Apply the naming rule to a Node row's name slot (latest own wins).
    pub(crate) fn adopt_name(&self, slot: &mut String) {
        if self.own_name || slot.is_empty() {
            *slot = self.name.clone();
        }
    }
}

impl RunIdentity {
    pub(crate) fn for_run(mut self, run_id: &str, started_at: &str) -> Self {
        self.run_id = run_id.to_string();
        self.started_at = started_at.to_string();
        self
    }

    /// Apply the naming rule to a row's name slot (folds walk Runs oldest first,
    /// so « latest own Run wins » is a plain overwrite).
    pub(crate) fn adopt_name(&self, slot: &mut String) {
        if self.own_name || slot.is_empty() {
            *slot = self.pipeline_name.clone();
        }
    }

    pub(crate) fn node_name(&self, node_id: &str) -> String {
        self.node_defs
            .get(node_id)
            .map(|def| def.name.clone())
            .unwrap_or_else(|| node_id.to_string())
    }

    /// Node `raw` of this Run, as the Pipeline row counts it.
    pub(crate) fn node(&self, raw: &str) -> NodeIdentity {
        let key = self.nodes.resolve(raw).to_string();
        let own_name = key == raw;
        let name = if own_name {
            self.node_name(raw)
        } else {
            self.nodes
                .stored_name(&key)
                .map(str::to_string)
                .unwrap_or_else(|| self.node_name(&key))
        };
        NodeIdentity {
            key,
            raw_key: raw.to_string(),
            name,
            own_name,
        }
    }

    /// The model id model `raw` is counted under (step 2).
    pub(crate) fn model(&self, raw: &str) -> String {
        self.global.models.resolve(raw).to_string()
    }

    /// The effort the « By model » axis counts `raw_effort` of raw model
    /// `raw_model` under (step 1, on the raw model — ADR-0078).
    pub(crate) fn effort(&self, raw_model: &str, raw_effort: Option<&str>) -> Option<String> {
        self.global.effort(raw_model, raw_effort)
    }

    /// The couple `raw_model` × `raw_effort` of Node `raw_node` of this Run is
    /// counted as (steps 1–4). `raw_node` is `None` on a row that is no Node:
    /// only the global steps apply there.
    pub(crate) fn couple(
        &self,
        raw_node: Option<&str>,
        raw_model: &str,
        raw_effort: Option<&str>,
    ) -> CoupleIdentity {
        let raw_key = couple_key(raw_model, raw_effort);
        let (model, effort) = self.global.couple(raw_model, raw_effort);
        let global_key = couple_key(&model, effort.as_deref());
        let scope = raw_node
            .map(|node| couple_scope(&self.raw_pipeline_key, node))
            .unwrap_or_default();
        let local = self
            .couples
            .get(&scope)
            .and_then(|couples| couples.to.get(&global_key))
            .and_then(|key| split_couple_key(key).map(|split| (key.clone(), split)));
        let (key, model, effort) = match local {
            Some((key, (model, effort))) => (key, model, effort),
            None => (global_key.clone(), model, effort),
        };
        CoupleIdentity {
            model,
            effort,
            key,
            global: global_key != raw_key,
            global_key,
            raw_key,
            scope,
        }
    }
}

// --- What a row says about its members -------------------------------------------

/// What one raw key did in the period, under the row that counts it.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyTally {
    runs: BTreeSet<String>,
    executions: u64,
    last_run: Option<String>,
    observed: u64,
    requested: u64,
}

/// The per-raw-key tallies of one row (a Pipeline, a Node or a model): the
/// row's own `runs` / `last_run`, and what each absorbed member contributed.
#[derive(Debug, Clone, Default)]
pub(crate) struct PipelineTallies {
    by_key: BTreeMap<String, KeyTally>,
    count_executions: bool,
}

impl PipelineTallies {
    /// Sessions counts executions; Cost and Performance count Runs only.
    pub(crate) fn counting_executions() -> Self {
        Self {
            count_executions: true,
            ..Self::default()
        }
    }

    pub(crate) fn record_run(&mut self, identity: &RunIdentity, run_id: &str, started_at: &str) {
        self.record_run_as(&identity.raw_pipeline_key, run_id, started_at);
    }

    pub(crate) fn record_execution(&mut self, identity: &RunIdentity) {
        self.record_execution_as(&identity.raw_pipeline_key);
    }

    /// A Run seen under raw key `key` (a Pipeline key, a Node id or a model id).
    pub(crate) fn record_run_as(&mut self, key: &str, run_id: &str, started_at: &str) {
        let tally = self.by_key.entry(key.to_string()).or_default();
        tally.runs.insert(run_id.to_string());
        if tally
            .last_run
            .as_deref()
            .is_none_or(|last| last < started_at)
        {
            tally.last_run = Some(started_at.to_string());
        }
    }

    pub(crate) fn record_execution_as(&mut self, key: &str) {
        self.by_key.entry(key.to_string()).or_default().executions += 1;
    }

    /// One observation of model `key`, read from the source or requested — only
    /// model rows record it (#892), so only their members carry a provenance.
    pub(crate) fn record_provenance_as(&mut self, key: &str, observed: bool) {
        let tally = self.by_key.entry(key.to_string()).or_default();
        if observed {
            tally.observed += 1;
        } else {
            tally.requested += 1;
        }
    }

    pub(crate) fn runs(&self) -> u64 {
        self.by_key
            .values()
            .flat_map(|t| t.runs.iter())
            .collect::<BTreeSet<_>>()
            .len() as u64
    }

    pub(crate) fn last_run(&self) -> Option<String> {
        self.by_key
            .values()
            .filter_map(|t| t.last_run.clone())
            .max()
    }

    /// The raw keys seen (a model row's raw model ids…).
    pub(crate) fn keys(&self) -> impl Iterator<Item = &str> {
        self.by_key.keys().map(String::as_str)
    }

    /// The raw couples a couple row counts through a global absorption (#906),
    /// as its read-only « Global absorption » list shows them. `self` is keyed
    /// by raw couple key, and only records the moved ones.
    pub(crate) fn global_members(&self) -> Vec<AbsorbedMember> {
        self.by_key
            .keys()
            .map(|key| {
                let name = split_couple_key(key)
                    .map(|(model, effort)| couple_name(&model, effort.as_deref()))
                    .unwrap_or_else(|| key.clone());
                self.member(&Named {
                    key: key.clone(),
                    name,
                })
            })
            .collect()
    }

    fn member(&self, member: &Named) -> AbsorbedMember {
        let tally = self.by_key.get(&member.key);
        AbsorbedMember {
            key: member.key.clone(),
            name: member.name.clone(),
            runs: tally.map_or(0, |t| t.runs.len() as u64),
            executions: self
                .count_executions
                .then(|| tally.map_or(0, |t| t.executions)),
            last_run: tally.and_then(|t| t.last_run.clone()),
            provenance: tally
                .filter(|t| t.observed + t.requested > 0)
                .map(|t| crate::stats::provenance(t.observed as i64, t.requested as i64)),
            scopes: Vec::new(),
        }
    }
}

/// One absorbed member as a row lists it: its name, and what it did in the
/// period (`runs` always; `executions` on Sessions rows only; `provenance` on
/// model rows only — where its id was read from, ADR-0065 §1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorbedMember {
    pub key: String,
    pub name: String,
    pub runs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::stats::StatsProvenance>,
    /// The raw scopes whose absorption holds this member — on effort and
    /// couple rows only (#906): their ✕ takes it out of every one of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

// --- HTTP ----------------------------------------------------------------------

/// One absorption as `GET /stats/absorptions` lists it: the absorbent and its
/// members, names beside keys (the UI shows the names only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorptionView {
    pub dimension: String,
    pub scope: String,
    /// A Node absorption's Pipeline, by name (#892) — empty on the others.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope_name: String,
    pub absorbent: Named,
    pub members: Vec<AbsorptionMemberView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorptionMemberView {
    pub key: String,
    pub name: String,
    pub origin: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorptionList {
    pub absorptions: Vec<AbsorptionView>,
}

pub(crate) fn group(rows: Vec<AbsorptionRow>) -> AbsorptionList {
    let mut grouped: BTreeMap<(String, String, String), AbsorptionView> = BTreeMap::new();
    for row in rows {
        let view = grouped
            .entry((
                row.dimension.clone(),
                row.scope.clone(),
                row.absorbent.clone(),
            ))
            .or_insert_with(|| AbsorptionView {
                dimension: row.dimension.clone(),
                scope: row.scope.clone(),
                scope_name: row.scope_name.clone(),
                absorbent: Named {
                    key: row.absorbent.clone(),
                    name: row.absorbent_name.clone(),
                },
                members: Vec::new(),
            });
        view.members.push(AbsorptionMemberView {
            key: row.member,
            name: row.member_name,
            origin: row.origin,
            created_at: row.created_at,
        });
    }
    AbsorptionList {
        absorptions: grouped.into_values().collect(),
    }
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

async fn current(db: &SqlitePool) -> Response {
    match list(db).await {
        Ok(rows) => Json(group(rows)).into_response(),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("stats absorptions failed: {e}"),
        ),
    }
}

/// `GET /stats/absorptions`.
pub(crate) async fn list_absorptions(State(state): State<Arc<AppState>>) -> Response {
    current(&state.db).await
}

/// One row of a Combine request. `scope` names the Pipeline row a Node was
/// selected under (#892): the daemon refuses Nodes of two different Pipelines
/// rather than trust the request's single `scope`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CombineRow {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub scope: Option<String>,
}

impl CombineRow {
    fn named(&self) -> Named {
        Named {
            key: self.key.clone(),
            name: self.name.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CombineRequest {
    pub dimension: String,
    /// The key of the Pipeline row a Node absorption is scoped to; empty on the
    /// other dimensions.
    #[serde(default)]
    pub scope: String,
    /// That Pipeline's name, for Settings.
    #[serde(default)]
    pub scope_name: String,
    pub absorbent: CombineRow,
    pub members: Vec<CombineRow>,
}

/// The refusal for a dimension Stats does not combine, if any.
fn unsupported_dimension(dimension: &str) -> Option<Response> {
    (![PIPELINE, NODE, MODEL, EFFORT, COUPLE].contains(&dimension)).then(|| {
        error(
            StatusCode::BAD_REQUEST,
            format!("unsupported absorption dimension: {dimension}"),
        )
    })
}

/// The refusal for a scope that does not fit the dimension, if any: a Node
/// absorption lives under ONE Pipeline row — one that is not itself counted
/// under another — and the other dimensions have no scope.
async fn invalid_scope(
    db: &SqlitePool,
    body: &CombineRequest,
) -> Result<Option<Response>, sqlx::Error> {
    match body.dimension.as_str() {
        EFFORT => return invalid_effort_scope(db, body).await,
        COUPLE => return invalid_couple_scope(db, body).await,
        _ => {}
    }
    if body.dimension != NODE {
        return Ok((!body.scope.is_empty()).then(|| {
            error(
                StatusCode::BAD_REQUEST,
                format!("a {} absorption has no scope", body.dimension),
            )
        }));
    }
    if body.scope.trim().is_empty() {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "a node absorption needs the pipeline its nodes run under",
        )));
    }
    let rows = std::iter::once(&body.absorbent).chain(body.members.iter());
    if rows
        .filter_map(|row| row.scope.as_deref())
        .any(|scope| scope != body.scope)
    {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "nodes of different pipelines cannot be combined",
        )));
    }
    let absorbed: Option<(String,)> = sqlx::query_as(
        "SELECT absorbent FROM stats_absorptions WHERE dimension = ? AND scope = '' AND member = ?",
    )
    .bind(PIPELINE)
    .bind(&body.scope)
    .fetch_optional(db)
    .await?;
    Ok(absorbed.map(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "this pipeline is combined into another: combine its nodes there",
        )
    }))
}

/// Whether every row of the request was selected under the request's scope.
fn rows_share_the_scope(body: &CombineRequest) -> bool {
    std::iter::once(&body.absorbent)
        .chain(body.members.iter())
        .filter_map(|row| row.scope.as_deref())
        .all(|scope| scope == body.scope)
}

async fn absorbent_of(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    member: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT absorbent FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?",
    )
    .bind(dimension)
    .bind(scope)
    .bind(member)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(key,)| key))
}

/// An effort absorption lives in ONE model row (grilling Q1): efforts of two
/// models are refused, and so is a model row counted under another.
async fn invalid_effort_scope(
    db: &SqlitePool,
    body: &CombineRequest,
) -> Result<Option<Response>, sqlx::Error> {
    if body.scope.trim().is_empty() {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "an effort absorption needs the model its efforts ran on",
        )));
    }
    if !rows_share_the_scope(body) {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "efforts of different models cannot be combined",
        )));
    }
    Ok(absorbent_of(db, MODEL, "", &body.scope).await?.map(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "this model is combined into another: combine its efforts there",
        )
    }))
}

/// A couple absorption lives in ONE Node row (grilling Q4): couples of two
/// Nodes are refused, and so is a Node row counted under another.
async fn invalid_couple_scope(
    db: &SqlitePool,
    body: &CombineRequest,
) -> Result<Option<Response>, sqlx::Error> {
    let Some((pipeline, node)) = parse_couple_scope(&body.scope) else {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "a couple absorption needs the pipeline and the node its couples ran under",
        )));
    };
    if !rows_share_the_scope(body) {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "couples of different nodes cannot be combined",
        )));
    }
    if let Some(row) = std::iter::once(&body.absorbent)
        .chain(body.members.iter())
        .find(|row| split_couple_key(&row.key).is_none())
    {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            format!("not a model × effort couple: {}", row.name),
        )));
    }
    if absorbent_of(db, PIPELINE, "", &pipeline).await?.is_some() {
        return Ok(Some(error(
            StatusCode::BAD_REQUEST,
            "this pipeline is combined into another: combine its couples there",
        )));
    }
    Ok(absorbent_of(db, NODE, &pipeline, &node).await?.map(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "this node is combined into another: combine its couples there",
        )
    }))
}

/// `POST /stats/absorptions` — create or extend an absorption. Answers the
/// whole list, as it stands after the write.
pub(crate) async fn create_absorption(
    State(state): State<Arc<AppState>>,
    actor: crate::audit_log::Actor,
    Json(body): Json<CombineRequest>,
) -> Response {
    if let Some(response) = unsupported_dimension(&body.dimension) {
        return response;
    }
    match invalid_scope(&state.db, &body).await {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(e) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("stats absorption failed: {e}"),
            )
        }
    }
    // « not set » is the effort key `""`: an effort row may be keyed by it.
    let empty_allowed = body.dimension == EFFORT;
    if !empty_allowed && body.absorbent.key.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "absorbent key is empty");
    }
    let absorbent = body.absorbent.named();
    let members: Vec<Named> = body
        .members
        .iter()
        .filter(|m| m.key != absorbent.key)
        .map(CombineRow::named)
        .collect();
    if members.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "an absorption needs at least one other member",
        );
    }
    if !empty_allowed && members.iter().any(|m| m.key.trim().is_empty()) {
        return error(StatusCode::BAD_REQUEST, "member key is empty");
    }
    let written = if [EFFORT, COUPLE].contains(&body.dimension.as_str()) {
        combine_frozen(
            &state.db,
            &body.dimension,
            &body.scope,
            &body.scope_name,
            &absorbent,
            &members,
            ORIGIN_MANUAL,
        )
        .await
    } else {
        combine_scoped(
            &state.db,
            Scope {
                dimension: &body.dimension,
                key: &body.scope,
                name: &body.scope_name,
            },
            &absorbent,
            &members,
            ORIGIN_MANUAL,
        )
        .await
    };
    if let Err(e) = written {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("stats absorption failed: {e}"),
        );
    }
    crate::audit_log::record_best_effort(
        &state.db,
        crate::audit_log::NewAuditEntry {
            actor_hint: actor.as_hint().to_string(),
            action: "stats_absorption.combine".to_string(),
            target_kind: Some("stats_absorption".to_string()),
            target_id: Some(absorbent.key.clone()),
            before: None,
            after: Some(serde_json::json!({
                "dimension": body.dimension,
                "scope": body.scope,
                "absorbent": absorbent,
                "members": members,
            })),
        },
    )
    .await;
    current(&state.db).await
}

#[derive(Debug, Deserialize)]
pub(crate) struct UncombineQuery {
    pub dimension: String,
    #[serde(default)]
    pub scope: String,
    pub member: String,
}

/// `DELETE /stats/absorptions/member?dimension=&scope=&member=` — the ✕ of the
/// members list. The key travels in the query string: a Pipeline key is a
/// file-name stem or a free name, never safe as a path segment.
pub(crate) async fn delete_absorption_member(
    State(state): State<Arc<AppState>>,
    actor: crate::audit_log::Actor,
    Query(q): Query<UncombineQuery>,
) -> Response {
    if let Some(response) = unsupported_dimension(&q.dimension) {
        return response;
    }
    match remove_member(&state.db, &q.dimension, &q.scope, &q.member).await {
        Ok(true) => {
            crate::audit_log::record_best_effort(
                &state.db,
                crate::audit_log::NewAuditEntry {
                    actor_hint: actor.as_hint().to_string(),
                    action: "stats_absorption.uncombine".to_string(),
                    target_kind: Some("stats_absorption".to_string()),
                    target_id: Some(q.member.clone()),
                    before: Some(serde_json::json!({
                        "dimension": q.dimension,
                        "scope": q.scope,
                        "member": q.member,
                    })),
                    after: None,
                },
            )
            .await;
            current(&state.db).await
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "this row is not combined"),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("stats absorption failed: {e}"),
        ),
    }
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
        crate::init_db(&db).await.unwrap();
        db
    }

    fn named(key: &str) -> Named {
        Named {
            key: key.to_string(),
            name: key.to_uppercase(),
        }
    }

    async fn combine_manual(db: &SqlitePool, absorbent: &str, members: &[&str]) {
        let members: Vec<Named> = members.iter().map(|m| named(m)).collect();
        combine(db, PIPELINE, "", &named(absorbent), &members, ORIGIN_MANUAL)
            .await
            .unwrap();
    }

    /// member → absorbent, sorted — the whole observable state of the store.
    async fn edges(db: &SqlitePool) -> Vec<(String, String)> {
        let mut edges: Vec<(String, String)> = list(db)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.member, row.absorbent))
            .collect();
        edges.sort();
        edges
    }

    fn edge(member: &str, absorbent: &str) -> (String, String) {
        (member.to_string(), absorbent.to_string())
    }

    #[tokio::test]
    async fn absorbing_an_absorbent_moves_its_members_and_stays_one_level() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        combine_manual(&db, "c", &["b"]).await;
        assert_eq!(edges(&db).await, vec![edge("a", "c"), edge("b", "c")]);

        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        assert_eq!(resolver.pipeline("a"), "c");
        assert_eq!(resolver.pipeline("b"), "c");
        assert_eq!(resolver.pipeline("c"), "c");
    }

    #[tokio::test]
    async fn a_member_has_exactly_one_absorbent() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        combine_manual(&db, "c", &["a"]).await;
        assert_eq!(edges(&db).await, vec![edge("a", "c")]);
    }

    #[tokio::test]
    async fn choosing_a_member_as_the_absorbent_takes_it_out_first() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a", "x"]).await;
        // The operator selects `a` and `b` and keeps the name of `a`.
        combine_manual(&db, "a", &["b"]).await;
        assert_eq!(edges(&db).await, vec![edge("b", "a"), edge("x", "a")]);
    }

    #[tokio::test]
    async fn two_absorbents_merge_under_the_one_chosen() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        combine_manual(&db, "d", &["c"]).await;
        combine_manual(&db, "d", &["b"]).await;
        assert_eq!(
            edges(&db).await,
            vec![edge("a", "d"), edge("b", "d"), edge("c", "d")]
        );
        let list = group(list(&db).await.unwrap());
        assert_eq!(list.absorptions.len(), 1);
        assert_eq!(list.absorptions[0].absorbent, named("d"));
    }

    async fn rename(db: &SqlitePool, old: &str, new: &str) {
        record_rename(db, &named(old), &named(new)).await.unwrap();
    }

    #[tokio::test]
    async fn a_rename_chain_ends_as_one_absorbent_with_every_old_key() {
        let db = mem_db().await;
        rename(&db, "a", "b").await;
        rename(&db, "b", "c").await;
        assert_eq!(edges(&db).await, vec![edge("a", "c"), edge("b", "c")]);
        let rows = list(&db).await.unwrap();
        assert!(rows.iter().all(|row| row.origin == ORIGIN_RENAME));
        assert!(rows.iter().all(|row| row.absorbent_name == "C"));
    }

    #[tokio::test]
    async fn renaming_back_swaps_the_absorbent() {
        let db = mem_db().await;
        rename(&db, "a", "b").await;
        rename(&db, "b", "a").await;
        assert_eq!(edges(&db).await, vec![edge("b", "a")]);
    }

    #[tokio::test]
    async fn renaming_an_absorbed_pipeline_joins_its_absorbent() {
        let db = mem_db().await;
        combine_manual(&db, "keeper", &["old"]).await;
        rename(&db, "old", "new").await;
        assert_eq!(
            edges(&db).await,
            vec![edge("new", "keeper"), edge("old", "keeper")]
        );
        let origins: Vec<(String, String)> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.member, row.origin))
            .collect();
        assert!(origins.contains(&("old".to_string(), ORIGIN_MANUAL.to_string())));
        assert!(origins.contains(&("new".to_string(), ORIGIN_RENAME.to_string())));
    }

    #[tokio::test]
    async fn renaming_a_manual_absorbent_keeps_its_members_manual() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        rename(&db, "b", "c").await;
        let rows = list(&db).await.unwrap();
        let origin = |member: &str| {
            rows.iter()
                .find(|row| row.member == member)
                .map(|row| row.origin.clone())
                .unwrap()
        };
        assert_eq!(edges(&db).await, vec![edge("a", "c"), edge("b", "c")]);
        assert_eq!(origin("a"), ORIGIN_MANUAL);
        assert_eq!(origin("b"), ORIGIN_RENAME);
    }

    #[tokio::test]
    async fn removing_the_last_member_removes_the_absorption() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        assert!(remove_member(&db, PIPELINE, "", "a").await.unwrap());
        assert!(list(&db).await.unwrap().is_empty());
        assert!(!remove_member(&db, PIPELINE, "", "a").await.unwrap());
    }

    #[tokio::test]
    async fn the_fingerprint_moves_with_every_write() {
        let db = mem_db().await;
        let empty = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        combine_manual(&db, "b", &["a"]).await;
        let one = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        combine_manual(&db, "b", &["x"]).await;
        let two = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        remove_member(&db, PIPELINE, "", "x").await.unwrap();
        let back = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        assert_ne!(empty, one);
        assert_ne!(one, two);
        assert_eq!(one, back);
    }

    #[tokio::test]
    async fn uncombined_reads_through_an_empty_resolver() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        let resolver = AbsorptionResolver::load(&db, true).await.unwrap();
        assert_eq!(resolver.pipeline("a"), "a");
    }

    #[test]
    fn an_absorbed_run_takes_the_absorbents_stored_name() {
        let rows = vec![AbsorptionRow {
            dimension: PIPELINE.to_string(),
            scope: String::new(),
            scope_name: String::new(),
            member: "old".to_string(),
            member_name: "Old".to_string(),
            absorbent: "new".to_string(),
            absorbent_name: "New".to_string(),
            origin: ORIGIN_MANUAL.to_string(),
            created_at: "2026-09-24T00:00:00Z".to_string(),
        }];
        let resolver = AbsorptionResolver::from_rows(&rows);
        let absorbed = resolver
            .run_identity(&serde_json::json!({"pipeline_id": "old", "pipeline_name": "Old"}));
        assert_eq!(absorbed.pipeline_key, "new");
        assert_eq!(absorbed.pipeline_name, "New");
        assert!(!absorbed.own_name);
        let own = resolver
            .run_identity(&serde_json::json!({"pipeline_id": "new", "pipeline_name": "New v2"}));
        // An absorbed Run seen first names the row; the absorbent's own Run wins.
        let mut slot = String::new();
        absorbed.adopt_name(&mut slot);
        assert_eq!(slot, "New");
        own.adopt_name(&mut slot);
        assert_eq!(slot, "New v2");
        absorbed.adopt_name(&mut slot);
        assert_eq!(slot, "New v2");
    }

    #[test]
    fn the_identity_falls_back_from_id_to_name_to_unknown() {
        assert_eq!(
            pipeline_identity(&serde_json::json!({"pipeline_name": "Only name"})),
            ("Only name".to_string(), "Only name".to_string())
        );
        assert_eq!(
            pipeline_identity(&serde_json::json!({})),
            (UNKNOWN_PIPELINE.to_string(), UNKNOWN_PIPELINE.to_string())
        );
    }

    // --- Nodes and Models (#892) ---

    async fn combine_nodes(db: &SqlitePool, scope: &str, absorbent: &str, members: &[&str]) {
        let members: Vec<Named> = members.iter().map(|m| named(m)).collect();
        combine_scoped(
            db,
            Scope {
                dimension: NODE,
                key: scope,
                name: &scope.to_uppercase(),
            },
            &named(absorbent),
            &members,
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
    }

    /// (scope, member → absorbent) of every Node row, sorted.
    async fn node_edges(db: &SqlitePool) -> Vec<(String, String, String)> {
        let mut edges: Vec<(String, String, String)> = list(db)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.dimension == NODE)
            .map(|row| (row.scope, row.member, row.absorbent))
            .collect();
        edges.sort();
        edges
    }

    fn node_edge(scope: &str, member: &str, absorbent: &str) -> (String, String, String) {
        (scope.to_string(), member.to_string(), absorbent.to_string())
    }

    /// A Run of `pipeline` that started Node `node` — what the cascade reads.
    async fn ran(db: &SqlitePool, run: &str, pipeline: &str, nodes: &[&str]) {
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, 'run_started', ?)",
        )
        .bind(run)
        .bind("2026-09-24T00:00:00Z")
        .bind(serde_json::json!({"pipeline_id": pipeline, "pipeline_name": pipeline}).to_string())
        .execute(db)
        .await
        .unwrap();
        for node in nodes {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, 'node_started', ?, 1, '{}')",
            )
            .bind(run)
            .bind("2026-09-24T00:01:00Z")
            .bind(node)
            .execute(db)
            .await
            .unwrap();
        }
    }

    fn run_of(pipeline: &str) -> serde_json::Value {
        serde_json::json!({"pipeline_id": pipeline, "pipeline_name": pipeline})
    }

    #[tokio::test]
    async fn a_node_absorption_applies_to_the_runs_of_its_pipeline_row_only() {
        let db = mem_db().await;
        combine_manual(&db, "b", &["a"]).await;
        combine_nodes(&db, "b", "code-review", &["review"]).await;
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());

        // A's Run counts under B, so B's Node absorption applies to it.
        let node = resolver.run_identity(&run_of("a")).node("review");
        assert_eq!(node.key, "code-review");
        assert_eq!(node.raw_key, "review");
        assert_eq!(node.name, "CODE-REVIEW", "the absorbent's stored name");
        assert!(!node.own_name);
        assert_eq!(
            resolver.run_identity(&run_of("b")).node("review").key,
            "code-review"
        );
        // Another Pipeline's `review` is not touched.
        assert_eq!(
            resolver.run_identity(&run_of("c")).node("review").key,
            "review"
        );
        // The absorbent Node keeps its own name.
        let own = resolver.run_identity(&run_of("b")).node("code-review");
        assert!(own.own_name);
    }

    #[tokio::test]
    async fn a_pipeline_taken_out_takes_its_nodes_out_of_the_node_absorption() {
        let db = mem_db().await;
        // `review` only ever ran under A; `code-review` under B; `shared` under both.
        ran(&db, "r-a", "a", &["review", "shared"]).await;
        ran(&db, "r-b", "b", &["code-review", "shared"]).await;
        combine_manual(&db, "b", &["a"]).await;
        combine_nodes(&db, "b", "code-review", &["review"]).await;
        combine_nodes(&db, "b", "shared-new", &["shared"]).await;

        // The resolver alone already reads A's Nodes under their own ids once
        // A is out (read-time cascade)…
        let without_a: Vec<AbsorptionRow> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| !(row.dimension == PIPELINE && row.member == "a"))
            .collect();
        let resolver = AbsorptionResolver::from_rows(&without_a);
        assert_eq!(
            resolver.run_identity(&run_of("a")).node("review").key,
            "review"
        );

        // …and the store drops the Node rows that only A's Runs explained.
        assert!(remove_member(&db, PIPELINE, "", "a").await.unwrap());
        assert_eq!(
            node_edges(&db).await,
            vec![node_edge("b", "shared", "shared-new")],
            "`shared` still runs under B: its absorption stays"
        );
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        assert_eq!(
            resolver.run_identity(&run_of("b")).node("shared").key,
            "shared-new"
        );
        assert_eq!(
            resolver.run_identity(&run_of("a")).node("review").key,
            "review"
        );
    }

    #[tokio::test]
    async fn a_node_absorbent_that_only_ran_under_the_removed_pipeline_dissolves() {
        let db = mem_db().await;
        ran(&db, "r-a", "a", &["writer-v2"]).await;
        ran(&db, "r-b", "b", &["writer"]).await;
        combine_manual(&db, "b", &["a"]).await;
        combine_nodes(&db, "b", "writer-v2", &["writer"]).await;
        remove_member(&db, PIPELINE, "", "a").await.unwrap();
        assert!(node_edges(&db).await.is_empty());
    }

    #[tokio::test]
    async fn combining_pipelines_carries_the_members_node_absorptions_over() {
        let db = mem_db().await;
        combine_nodes(&db, "a", "y", &["x"]).await;
        combine_nodes(&db, "b", "z", &["y"]).await;
        combine_manual(&db, "b", &["a"]).await;
        // A's `x → y` moves under B, where `y` is already absorbed by `z`: one
        // level, so `x` joins `z`.
        assert_eq!(
            node_edges(&db).await,
            vec![node_edge("b", "x", "z"), node_edge("b", "y", "z")]
        );
        let rows = list(&db).await.unwrap();
        assert!(rows
            .iter()
            .filter(|row| row.dimension == NODE)
            .all(|row| row.scope_name == "B"));
    }

    #[tokio::test]
    async fn a_rename_keeps_the_node_absorptions_of_the_pipeline() {
        let db = mem_db().await;
        combine_nodes(&db, "old", "code-review", &["review"]).await;
        rename(&db, "old", "new").await;
        assert_eq!(
            node_edges(&db).await,
            vec![node_edge("new", "review", "code-review")]
        );
    }

    #[tokio::test]
    async fn a_model_absorption_only_folds_the_model_id() {
        let db = mem_db().await;
        let absorbent = Named {
            key: "claude-opus-4-8".to_string(),
            name: "claude-opus-4-8".to_string(),
        };
        let alias = Named {
            key: "opus".to_string(),
            name: "opus".to_string(),
        };
        combine(&db, MODEL, "", &absorbent, &[alias], ORIGIN_MANUAL)
            .await
            .unwrap();
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        let identity = resolver.run_identity(&run_of("p"));
        assert_eq!(identity.model("opus"), "claude-opus-4-8");
        assert_eq!(identity.model("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(
            identity.pipeline_key, "p",
            "the Pipeline axis is not touched"
        );
        assert_eq!(identity.node("opus").key, "opus");

        let mut tallies = PipelineTallies::default();
        tallies.record_run_as("opus", "r1", "2026-09-24T00:00:00Z");
        tallies.record_provenance_as("opus", false);
        let members = resolver.absorbed_models("claude-opus-4-8", &tallies);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].runs, 1);
        assert_eq!(
            members[0].provenance,
            Some(crate::stats::StatsProvenance::Requested),
            "the member says where its id was read from"
        );
    }

    fn model(key: &str) -> Named {
        Named {
            key: key.to_string(),
            name: key.to_string(),
        }
    }

    /// (dimension, scope, member, absorbent) of the effort and couple rows.
    async fn frozen_rows(db: &SqlitePool) -> Vec<(String, String, String, String)> {
        let mut rows: Vec<_> = list(db)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| [EFFORT, COUPLE].contains(&row.dimension.as_str()))
            .map(|row| (row.dimension, row.scope, row.member, row.absorbent))
            .collect();
        rows.sort();
        rows
    }

    #[tokio::test]
    async fn an_effort_absorption_is_written_once_per_raw_model_it_covers() {
        let db = mem_db().await;
        combine(&db, MODEL, "", &model("m55"), &[model("m5")], ORIGIN_MANUAL)
            .await
            .unwrap();
        combine_frozen(
            &db,
            EFFORT,
            "m55",
            "m55",
            &model("high"),
            &[model("")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        let row = |scope: &str| {
            (
                EFFORT.to_string(),
                scope.to_string(),
                String::new(),
                "high".to_string(),
            )
        };
        assert_eq!(frozen_rows(&db).await, vec![row("m5"), row("m55")]);

        // A model absorbed later is not covered; one taken out keeps its rows.
        combine(
            &db,
            MODEL,
            "",
            &model("m55"),
            &[model("m48")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        remove_member(&db, MODEL, "", "m5").await.unwrap();
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        let identity = resolver.run_identity(&run_of("p"));
        assert_eq!(identity.effort("m5", None).as_deref(), Some("high"));
        assert_eq!(identity.effort("m55", None).as_deref(), Some("high"));
        assert_eq!(identity.effort("m48", None), None, "not covered");
        assert_eq!(identity.effort("m5", Some("low")).as_deref(), Some("low"));
    }

    #[tokio::test]
    async fn a_couple_absorption_covers_the_raw_nodes_of_its_row_where_they_ran() {
        let db = mem_db().await;
        ran(&db, "r1", "p", &["coder", "implementer"]).await;
        ran(&db, "r2", "q", &["coder"]).await;
        combine_manual(&db, "p", &["q"]).await;
        combine_nodes(&db, "p", "implementer", &["coder"]).await;
        combine_frozen(
            &db,
            COUPLE,
            &couple_scope("p", "implementer"),
            "Implementer (P)",
            &model("m|high"),
            &[model("m|")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        let scopes: Vec<String> = frozen_rows(&db)
            .await
            .into_iter()
            .map(|(_, scope, _, _)| scope)
            .collect();
        assert_eq!(
            scopes,
            vec![
                couple_scope("p", "coder"),
                couple_scope("p", "implementer"),
                couple_scope("q", "coder"),
            ],
            "q never ran `implementer`: no row for it"
        );
        let names: Vec<String> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.dimension == COUPLE)
            .map(|row| row.scope_name)
            .collect();
        assert!(names.contains(&"Implementer (P)".to_string()), "{names:?}");
    }

    #[tokio::test]
    async fn a_couple_resolves_global_first_then_on_its_raw_node() {
        let db = mem_db().await;
        combine_frozen(
            &db,
            EFFORT,
            "m5",
            "m5",
            &model("high"),
            &[model("")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        combine_frozen(
            &db,
            COUPLE,
            &couple_scope("p", "coder"),
            "",
            &model("m5|high"),
            &[model("m48|")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        let identity = resolver.run_identity(&run_of("p"));
        let couple = identity.couple(Some("coder"), "m5", None);
        assert_eq!(couple.key, "m5|high");
        assert!(couple.global, "the effort absorption moved it");
        assert_eq!(couple.raw_key, "m5|");
        let local = identity.couple(Some("coder"), "m48", None);
        assert_eq!(local.key, "m5|high");
        assert!(!local.global);
        assert_eq!(local.global_key, "m48|");
        let elsewhere = identity.couple(Some("reviewer"), "m48", None);
        assert_eq!(elsewhere.key, "m48|", "local to its Node");
        assert_eq!(
            identity.couple(None, "m48", None).key,
            "m48|",
            "a row that is no Node only takes the global steps"
        );

        // A model absorption posed later carries the local absorbent along.
        combine(&db, MODEL, "", &model("m55"), &[model("m5")], ORIGIN_MANUAL)
            .await
            .unwrap();
        let resolver = AbsorptionResolver::from_rows(&list(&db).await.unwrap());
        let identity = resolver.run_identity(&run_of("p"));
        assert_eq!(identity.couple(Some("coder"), "m48", None).key, "m55|high");
        assert_eq!(identity.couple(Some("coder"), "m5", None).key, "m55|high");
        assert_eq!(identity.model("m5"), "m55");
    }

    #[tokio::test]
    async fn the_fingerprint_covers_efforts_and_couples() {
        let db = mem_db().await;
        let empty = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        combine_frozen(
            &db,
            EFFORT,
            "m",
            "m",
            &model("high"),
            &[model("")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        let effort = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        combine_frozen(
            &db,
            COUPLE,
            &couple_scope("p", "n"),
            "",
            &model("m|high"),
            &[model("x|")],
            ORIGIN_MANUAL,
        )
        .await
        .unwrap();
        let couple = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        assert_ne!(empty, effort);
        assert_ne!(effort, couple);
        remove_member(&db, COUPLE, &couple_scope("p", "n"), "x|")
            .await
            .unwrap();
        let back = AbsorptionResolver::from_rows(&list(&db).await.unwrap()).fingerprint();
        assert_eq!(effort, back);
    }

    #[test]
    fn couple_keys_and_scopes_round_trip() {
        assert_eq!(couple_key("m", None), "m|");
        assert_eq!(
            split_couple_key("m|high"),
            Some(("m".to_string(), Some("high".to_string())))
        );
        assert_eq!(split_couple_key("m|"), Some(("m".to_string(), None)));
        assert_eq!(split_couple_key("nope"), None);
        assert_eq!(couple_name("m", None), "m · not set");
        let scope = couple_scope("a \"b\"", "n");
        assert_eq!(
            parse_couple_scope(&scope),
            Some(("a \"b\"".to_string(), "n".to_string()))
        );
    }

    #[tokio::test]
    async fn an_absorption_survives_a_reopened_database() {
        let dir = std::env::temp_dir().join(format!("pdo-absorption-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}?mode=rwc", dir.join("pdo.db").display());
        {
            let db = SqlitePool::connect(&url).await.unwrap();
            crate::init_db(&db).await.unwrap();
            combine_manual(&db, "b", &["a"]).await;
            db.close().await;
        }
        let db = SqlitePool::connect(&url).await.unwrap();
        crate::init_db(&db).await.unwrap();
        assert_eq!(edges(&db).await, vec![edge("a", "b")]);
        db.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
