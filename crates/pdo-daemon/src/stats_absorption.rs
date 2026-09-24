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
//! #890 ships the `pipeline` dimension. The table already carries `dimension`
//! and `scope` so Nodes (scoped to their Pipeline) and Models (#892) and the
//! rename-born `origin` (#891) land without a migration.

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

/// The only dimension #890 accepts. `node` and `model` come with #892.
pub(crate) const PIPELINE: &str = "pipeline";

/// Where an absorption came from: the operator's Combine (`manual`) or a
/// Pipeline rename that changed its id (`rename`, #891).
pub(crate) const ORIGIN_MANUAL: &str = "manual";

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
    Ok(())
}

/// One stored member row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct AbsorptionRow {
    pub dimension: String,
    pub scope: String,
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
        "SELECT dimension, scope, member, member_name, absorbent, absorbent_name, origin, created_at \
         FROM stats_absorptions ORDER BY dimension, scope, absorbent, created_at, member",
    )
    .fetch_all(db)
    .await
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
pub(crate) async fn combine(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> Result<(), sqlx::Error> {
    let now = crate::event_log::now_iso();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?")
        .bind(dimension)
        .bind(scope)
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
        .bind(scope)
        .bind(&member.key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO stats_absorptions \
               (dimension, scope, member, member_name, absorbent, absorbent_name, origin, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (dimension, scope, member) DO UPDATE SET \
               member_name = excluded.member_name, absorbent = excluded.absorbent, \
               absorbent_name = excluded.absorbent_name, origin = excluded.origin, \
               created_at = excluded.created_at",
        )
        .bind(dimension)
        .bind(scope)
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
    .bind(scope)
    .bind(&absorbent.key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Take one member out of its absorption. Removing the last member leaves no
/// row for that absorbent — the absorption is gone, and its rows come back.
/// `false` when the key was not absorbed.
pub(crate) async fn remove_member(
    db: &SqlitePool,
    dimension: &str,
    scope: &str,
    member: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM stats_absorptions WHERE dimension = ? AND scope = ? AND member = ?",
    )
    .bind(dimension)
    .bind(scope)
    .bind(member)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

// --- The resolver ---------------------------------------------------------------

/// The absorptions of one Stats read. Empty under `uncombined=true`, which is
/// exactly « the rows as the event log wrote them ».
#[derive(Debug, Clone, Default)]
pub(crate) struct AbsorptionResolver {
    /// member → absorbent, for the `pipeline` dimension.
    pipelines: HashMap<String, String>,
    /// absorbent → the name stored at the last Combine.
    absorbent_names: HashMap<String, String>,
    /// absorbent → its members, in stored order.
    members: BTreeMap<String, Vec<Named>>,
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
            if row.dimension != PIPELINE {
                continue;
            }
            resolver
                .pipelines
                .insert(row.member.clone(), row.absorbent.clone());
            resolver
                .absorbent_names
                .insert(row.absorbent.clone(), row.absorbent_name.clone());
            resolver
                .members
                .entry(row.absorbent.clone())
                .or_default()
                .push(Named {
                    key: row.member.clone(),
                    name: row.member_name.clone(),
                });
        }
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
        self.pipelines.get(key).map(String::as_str).unwrap_or(key)
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
            self.absorbent_names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| key.clone())
        };
        RunIdentity {
            pipeline_key: key,
            pipeline_name: name,
            own_name: own,
            raw_pipeline_key: raw_key,
            node_defs: node_defs(payload),
        }
    }

    /// The members of `absorbent` as a row lists them, with what each did in the
    /// period — every member, including one with no Run in it.
    pub(crate) fn absorbed(
        &self,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.members
            .get(absorbent)
            .into_iter()
            .flatten()
            .map(|member| {
                let tally = tallies.by_key.get(&member.key);
                AbsorbedMember {
                    key: member.key.clone(),
                    name: member.name.clone(),
                    runs: tally.map_or(0, |t| t.runs.len() as u64),
                    executions: tallies
                        .count_executions
                        .then(|| tally.map_or(0, |t| t.executions)),
                    last_run: tally.and_then(|t| t.last_run.clone()),
                }
            })
            .collect()
    }
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
}

impl RunIdentity {
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
}

// --- What a row says about its members -------------------------------------------

/// What one raw Pipeline key did in the period, under the row that counts it.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyTally {
    runs: BTreeSet<String>,
    executions: u64,
    last_run: Option<String>,
}

/// The per-raw-key tallies of one Pipeline row: the row's own `runs` /
/// `last_run`, and what each absorbed member contributed.
#[derive(Debug, Clone, Default)]
pub(crate) struct PipelineTallies {
    by_key: BTreeMap<String, KeyTally>,
    count_executions: bool,
}

impl PipelineTallies {
    /// Sessions counts executions; Cost and Performance count Runs only.
    pub(crate) fn counting_executions() -> Self {
        Self {
            by_key: BTreeMap::new(),
            count_executions: true,
        }
    }

    pub(crate) fn record_run(&mut self, identity: &RunIdentity, run_id: &str, started_at: &str) {
        let tally = self
            .by_key
            .entry(identity.raw_pipeline_key.clone())
            .or_default();
        tally.runs.insert(run_id.to_string());
        if tally
            .last_run
            .as_deref()
            .is_none_or(|last| last < started_at)
        {
            tally.last_run = Some(started_at.to_string());
        }
    }

    pub(crate) fn record_execution(&mut self, identity: &RunIdentity) {
        self.by_key
            .entry(identity.raw_pipeline_key.clone())
            .or_default()
            .executions += 1;
    }

    pub(crate) fn runs(&self) -> u64 {
        self.by_key.values().map(|t| t.runs.len() as u64).sum()
    }

    pub(crate) fn last_run(&self) -> Option<String> {
        self.by_key
            .values()
            .filter_map(|t| t.last_run.clone())
            .max()
    }
}

/// One absorbed member as a Pipeline row lists it: its name, and what it did in
/// the period (`runs` always; `executions` on Sessions rows only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorbedMember {
    pub key: String,
    pub name: String,
    pub runs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
}

// --- HTTP ----------------------------------------------------------------------

/// One absorption as `GET /stats/absorptions` lists it: the absorbent and its
/// members, names beside keys (the UI shows the names only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AbsorptionView {
    pub dimension: String,
    pub scope: String,
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

#[derive(Debug, Deserialize)]
pub(crate) struct CombineRequest {
    pub dimension: String,
    #[serde(default)]
    pub scope: String,
    pub absorbent: Named,
    pub members: Vec<Named>,
}

/// The refusal for a dimension this build does not combine yet, if any.
fn unsupported_dimension(dimension: &str) -> Option<Response> {
    (dimension != PIPELINE).then(|| {
        error(
            StatusCode::BAD_REQUEST,
            format!("unsupported absorption dimension: {dimension}"),
        )
    })
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
    if body.absorbent.key.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "absorbent key is empty");
    }
    let members: Vec<Named> = body
        .members
        .into_iter()
        .filter(|m| m.key != body.absorbent.key)
        .collect();
    if members.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "an absorption needs at least one other member",
        );
    }
    if members.iter().any(|m| m.key.trim().is_empty()) {
        return error(StatusCode::BAD_REQUEST, "member key is empty");
    }
    if let Err(e) = combine(
        &state.db,
        &body.dimension,
        &body.scope,
        &body.absorbent,
        &members,
        ORIGIN_MANUAL,
    )
    .await
    {
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
            target_id: Some(body.absorbent.key.clone()),
            before: None,
            after: Some(serde_json::json!({
                "dimension": body.dimension,
                "absorbent": body.absorbent,
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
