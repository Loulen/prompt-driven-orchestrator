//! The absorptions' store over a real SQLite database, written through the
//! use cases the routes call: flattened on write, one absorbent per member,
//! the rename chain, the Node cascade read off the real event log, the frozen
//! scope of an effort or couple absorption, and the resolver read back from it
//! (moved from the legacy crate's in-crate tests, #928).

use pdo_kernel::Actor;
use pdo_stats::adapters::sqlite_absorptions::{init, SqliteAbsorptions};
use pdo_stats::application::absorptions::uncombine::{UncombineCommand, UncombineError};
use pdo_stats::application::ports::AbsorptionStore;
use pdo_stats::domain::absorption::list::group;
use pdo_stats::domain::absorption::request::{CombineRequest, CombineRow};
use pdo_stats::domain::absorption::resolver::AbsorptionResolver;
use pdo_stats::domain::absorption::tallies::PipelineTallies;
use pdo_stats::domain::absorption::{
    couple_scope, AbsorptionRow, Named, Scope, COUPLE, EFFORT, MODEL, NODE, ORIGIN_MANUAL,
    ORIGIN_RENAME, PIPELINE,
};
use pdo_stats::domain::provenance::StatsProvenance;
use sqlx::SqlitePool;

use crate::common::Harness;

fn row(named: &Named) -> CombineRow {
    CombineRow {
        key: named.key.clone(),
        name: named.name.clone(),
        scope: None,
    }
}

/// A Combine as the route asks for it, as the local operator.
async fn combine_frozen(
    h: &Harness,
    dimension: &str,
    scope: &str,
    scope_name: &str,
    absorbent: &Named,
    members: &[Named],
    _origin: &str,
) -> anyhow::Result<()> {
    h.stats
        .combine()
        .execute(
            &Actor::LocalOperator,
            "ui",
            CombineRequest {
                dimension: dimension.to_string(),
                scope: scope.to_string(),
                scope_name: scope_name.to_string(),
                absorbent: row(absorbent),
                members: members.iter().map(row).collect(),
            },
        )
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{e:?}"))
}

async fn combine(
    h: &Harness,
    dimension: &str,
    scope: &str,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> anyhow::Result<()> {
    combine_frozen(h, dimension, scope, "", absorbent, members, origin).await
}

async fn combine_scoped(
    h: &Harness,
    scope: Scope<'_>,
    absorbent: &Named,
    members: &[Named],
    origin: &str,
) -> anyhow::Result<()> {
    combine_frozen(
        h,
        scope.dimension,
        scope.key,
        scope.name,
        absorbent,
        members,
        origin,
    )
    .await
}

/// The ✕ of the members list; `false` when the key was not absorbed.
async fn remove_member(
    h: &Harness,
    dimension: &str,
    scope: &str,
    member: &str,
) -> anyhow::Result<bool> {
    let command = UncombineCommand {
        dimension: dimension.to_string(),
        scope: scope.to_string(),
        member: member.to_string(),
    };
    match h
        .stats
        .uncombine()
        .execute(&Actor::LocalOperator, "ui", command)
        .await
    {
        Ok(_) => Ok(true),
        Err(UncombineError::NotCombined) => Ok(false),
        Err(e) => Err(anyhow::anyhow!("{e:?}")),
    }
}

async fn record_rename(h: &Harness, old: &Named, new: &Named) -> anyhow::Result<()> {
    h.stats.record_rename().execute(old, new).await
}

fn named(key: &str) -> Named {
    Named {
        key: key.to_string(),
        name: key.to_uppercase(),
    }
}

async fn combine_manual(h: &Harness, absorbent: &str, members: &[&str]) {
    let members: Vec<Named> = members.iter().map(|m| named(m)).collect();
    combine(h, PIPELINE, "", &named(absorbent), &members, ORIGIN_MANUAL)
        .await
        .unwrap();
}

/// member → absorbent, sorted — the whole observable state of the store.
async fn edges(h: &Harness) -> Vec<(String, String)> {
    let mut edges: Vec<(String, String)> = h
        .absorption_rows()
        .await
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
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    combine_manual(&h, "c", &["b"]).await;
    assert_eq!(edges(&h).await, vec![edge("a", "c"), edge("b", "c")]);

    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
    assert_eq!(resolver.pipeline("a"), "c");
    assert_eq!(resolver.pipeline("b"), "c");
    assert_eq!(resolver.pipeline("c"), "c");
}

#[tokio::test]
async fn a_member_has_exactly_one_absorbent() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    combine_manual(&h, "c", &["a"]).await;
    assert_eq!(edges(&h).await, vec![edge("a", "c")]);
}

#[tokio::test]
async fn choosing_a_member_as_the_absorbent_takes_it_out_first() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a", "x"]).await;
    // The operator selects `a` and `b` and keeps the name of `a`.
    combine_manual(&h, "a", &["b"]).await;
    assert_eq!(edges(&h).await, vec![edge("b", "a"), edge("x", "a")]);
}

#[tokio::test]
async fn two_absorbents_merge_under_the_one_chosen() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    combine_manual(&h, "d", &["c"]).await;
    combine_manual(&h, "d", &["b"]).await;
    assert_eq!(
        edges(&h).await,
        vec![edge("a", "d"), edge("b", "d"), edge("c", "d")]
    );
    let list = group(h.absorption_rows().await);
    assert_eq!(list.absorptions.len(), 1);
    assert_eq!(list.absorptions[0].absorbent, named("d"));
}

async fn rename(h: &Harness, old: &str, new: &str) {
    record_rename(h, &named(old), &named(new)).await.unwrap();
}

#[tokio::test]
async fn a_rename_chain_ends_as_one_absorbent_with_every_old_key() {
    let h = Harness::new().await;
    rename(&h, "a", "b").await;
    rename(&h, "b", "c").await;
    assert_eq!(edges(&h).await, vec![edge("a", "c"), edge("b", "c")]);
    let rows = h.absorption_rows().await;
    assert!(rows.iter().all(|row| row.origin == ORIGIN_RENAME));
    assert!(rows.iter().all(|row| row.absorbent_name == "C"));
}

#[tokio::test]
async fn renaming_back_swaps_the_absorbent() {
    let h = Harness::new().await;
    rename(&h, "a", "b").await;
    rename(&h, "b", "a").await;
    assert_eq!(edges(&h).await, vec![edge("b", "a")]);
}

#[tokio::test]
async fn renaming_an_absorbed_pipeline_joins_its_absorbent() {
    let h = Harness::new().await;
    combine_manual(&h, "keeper", &["old"]).await;
    rename(&h, "old", "new").await;
    assert_eq!(
        edges(&h).await,
        vec![edge("new", "keeper"), edge("old", "keeper")]
    );
    let origins: Vec<(String, String)> = h
        .absorption_rows()
        .await
        .into_iter()
        .map(|row| (row.member, row.origin))
        .collect();
    assert!(origins.contains(&("old".to_string(), ORIGIN_MANUAL.to_string())));
    assert!(origins.contains(&("new".to_string(), ORIGIN_RENAME.to_string())));
}

#[tokio::test]
async fn renaming_a_manual_absorbent_keeps_its_members_manual() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    rename(&h, "b", "c").await;
    let rows = h.absorption_rows().await;
    let origin = |member: &str| {
        rows.iter()
            .find(|row| row.member == member)
            .map(|row| row.origin.clone())
            .unwrap()
    };
    assert_eq!(edges(&h).await, vec![edge("a", "c"), edge("b", "c")]);
    assert_eq!(origin("a"), ORIGIN_MANUAL);
    assert_eq!(origin("b"), ORIGIN_RENAME);
}

#[tokio::test]
async fn removing_the_last_member_removes_the_absorption() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    assert!(remove_member(&h, PIPELINE, "", "a").await.unwrap());
    assert!(h.absorption_rows().await.is_empty());
    assert!(!remove_member(&h, PIPELINE, "", "a").await.unwrap());
}

#[tokio::test]
async fn the_fingerprint_moves_with_every_write() {
    let h = Harness::new().await;
    let empty = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    combine_manual(&h, "b", &["a"]).await;
    let one = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    combine_manual(&h, "b", &["x"]).await;
    let two = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    remove_member(&h, PIPELINE, "", "x").await.unwrap();
    let back = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    assert_ne!(empty, one);
    assert_ne!(one, two);
    assert_eq!(one, back);
}

#[tokio::test]
async fn uncombined_reads_through_an_empty_resolver() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    let resolver = AbsorptionResolver::for_read(&h.absorption_rows().await, true);
    assert_eq!(resolver.pipeline("a"), "a");
}

// --- Nodes and Models (#892) ---

async fn combine_nodes(h: &Harness, scope: &str, absorbent: &str, members: &[&str]) {
    let members: Vec<Named> = members.iter().map(|m| named(m)).collect();
    combine_scoped(
        h,
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
async fn node_edges(h: &Harness) -> Vec<(String, String, String)> {
    let mut edges: Vec<(String, String, String)> = h
        .absorption_rows()
        .await
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
async fn ran(h: &Harness, run: &str, pipeline: &str, nodes: &[&str]) {
    sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, 'run_started', ?)")
        .bind(run)
        .bind("2026-09-24T00:00:00Z")
        .bind(serde_json::json!({"pipeline_id": pipeline, "pipeline_name": pipeline}).to_string())
        .execute(&h.db)
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
        .execute(&h.db)
        .await
        .unwrap();
    }
}

fn run_of(pipeline: &str) -> serde_json::Value {
    serde_json::json!({"pipeline_id": pipeline, "pipeline_name": pipeline})
}

#[tokio::test]
async fn a_node_absorption_applies_to_the_runs_of_its_pipeline_row_only() {
    let h = Harness::new().await;
    combine_manual(&h, "b", &["a"]).await;
    combine_nodes(&h, "b", "code-review", &["review"]).await;
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);

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
    let h = Harness::new().await;
    // `review` only ever ran under A; `code-review` under B; `shared` under both.
    ran(&h, "r-a", "a", &["review", "shared"]).await;
    ran(&h, "r-b", "b", &["code-review", "shared"]).await;
    combine_manual(&h, "b", &["a"]).await;
    combine_nodes(&h, "b", "code-review", &["review"]).await;
    combine_nodes(&h, "b", "shared-new", &["shared"]).await;

    // The resolver alone already reads A's Nodes under their own ids once
    // A is out (read-time cascade)…
    let without_a: Vec<AbsorptionRow> = h
        .absorption_rows()
        .await
        .into_iter()
        .filter(|row| !(row.dimension == PIPELINE && row.member == "a"))
        .collect();
    let resolver = AbsorptionResolver::from_rows(&without_a);
    assert_eq!(
        resolver.run_identity(&run_of("a")).node("review").key,
        "review"
    );

    // …and the store drops the Node rows that only A's Runs explained.
    assert!(remove_member(&h, PIPELINE, "", "a").await.unwrap());
    assert_eq!(
        node_edges(&h).await,
        vec![node_edge("b", "shared", "shared-new")],
        "`shared` still runs under B: its absorption stays"
    );
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
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
    let h = Harness::new().await;
    ran(&h, "r-a", "a", &["writer-v2"]).await;
    ran(&h, "r-b", "b", &["writer"]).await;
    combine_manual(&h, "b", &["a"]).await;
    combine_nodes(&h, "b", "writer-v2", &["writer"]).await;
    remove_member(&h, PIPELINE, "", "a").await.unwrap();
    assert!(node_edges(&h).await.is_empty());
}

#[tokio::test]
async fn combining_pipelines_carries_the_members_node_absorptions_over() {
    let h = Harness::new().await;
    combine_nodes(&h, "a", "y", &["x"]).await;
    combine_nodes(&h, "b", "z", &["y"]).await;
    combine_manual(&h, "b", &["a"]).await;
    // A's `x → y` moves under B, where `y` is already absorbed by `z`: one
    // level, so `x` joins `z`.
    assert_eq!(
        node_edges(&h).await,
        vec![node_edge("b", "x", "z"), node_edge("b", "y", "z")]
    );
    let rows = h.absorption_rows().await;
    assert!(rows
        .iter()
        .filter(|row| row.dimension == NODE)
        .all(|row| row.scope_name == "B"));
}

#[tokio::test]
async fn a_rename_keeps_the_node_absorptions_of_the_pipeline() {
    let h = Harness::new().await;
    combine_nodes(&h, "old", "code-review", &["review"]).await;
    rename(&h, "old", "new").await;
    assert_eq!(
        node_edges(&h).await,
        vec![node_edge("new", "review", "code-review")]
    );
}

#[tokio::test]
async fn a_model_absorption_only_folds_the_model_id() {
    let h = Harness::new().await;
    let absorbent = Named {
        key: "claude-opus-4-8".to_string(),
        name: "claude-opus-4-8".to_string(),
    };
    let alias = Named {
        key: "opus".to_string(),
        name: "opus".to_string(),
    };
    combine(&h, MODEL, "", &absorbent, &[alias], ORIGIN_MANUAL)
        .await
        .unwrap();
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
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
        Some(StatsProvenance::Requested),
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
async fn frozen_rows(h: &Harness) -> Vec<(String, String, String, String)> {
    let mut rows: Vec<_> = h
        .absorption_rows()
        .await
        .into_iter()
        .filter(|row| [EFFORT, COUPLE].contains(&row.dimension.as_str()))
        .map(|row| (row.dimension, row.scope, row.member, row.absorbent))
        .collect();
    rows.sort();
    rows
}

#[tokio::test]
async fn an_effort_absorption_is_written_once_per_raw_model_it_covers() {
    let h = Harness::new().await;
    combine(&h, MODEL, "", &model("m55"), &[model("m5")], ORIGIN_MANUAL)
        .await
        .unwrap();
    combine_frozen(
        &h,
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
    assert_eq!(frozen_rows(&h).await, vec![row("m5"), row("m55")]);

    // A model absorbed later is not covered; one taken out keeps its rows.
    combine(&h, MODEL, "", &model("m55"), &[model("m48")], ORIGIN_MANUAL)
        .await
        .unwrap();
    remove_member(&h, MODEL, "", "m5").await.unwrap();
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
    let identity = resolver.run_identity(&run_of("p"));
    assert_eq!(identity.effort("m5", None).as_deref(), Some("high"));
    assert_eq!(identity.effort("m55", None).as_deref(), Some("high"));
    assert_eq!(identity.effort("m48", None), None, "not covered");
    assert_eq!(identity.effort("m5", Some("low")).as_deref(), Some("low"));
}

#[tokio::test]
async fn a_couple_absorption_covers_the_raw_nodes_of_its_row_where_they_ran() {
    let h = Harness::new().await;
    ran(&h, "r1", "p", &["coder", "implementer"]).await;
    ran(&h, "r2", "q", &["coder"]).await;
    combine_manual(&h, "p", &["q"]).await;
    combine_nodes(&h, "p", "implementer", &["coder"]).await;
    combine_frozen(
        &h,
        COUPLE,
        &couple_scope("p", "implementer"),
        "Implementer (P)",
        &model("m|high"),
        &[model("m|")],
        ORIGIN_MANUAL,
    )
    .await
    .unwrap();
    let scopes: Vec<String> = frozen_rows(&h)
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
    let names: Vec<String> = h
        .absorption_rows()
        .await
        .into_iter()
        .filter(|row| row.dimension == COUPLE)
        .map(|row| row.scope_name)
        .collect();
    assert!(names.contains(&"Implementer (P)".to_string()), "{names:?}");
}

#[tokio::test]
async fn a_couple_resolves_global_first_then_on_its_raw_node() {
    let h = Harness::new().await;
    combine_frozen(
        &h,
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
        &h,
        COUPLE,
        &couple_scope("p", "coder"),
        "",
        &model("m5|high"),
        &[model("m48|")],
        ORIGIN_MANUAL,
    )
    .await
    .unwrap();
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
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
    combine(&h, MODEL, "", &model("m55"), &[model("m5")], ORIGIN_MANUAL)
        .await
        .unwrap();
    let resolver = AbsorptionResolver::from_rows(&h.absorption_rows().await);
    let identity = resolver.run_identity(&run_of("p"));
    assert_eq!(identity.couple(Some("coder"), "m48", None).key, "m55|high");
    assert_eq!(identity.couple(Some("coder"), "m5", None).key, "m55|high");
    assert_eq!(identity.model("m5"), "m55");
}

#[tokio::test]
async fn the_fingerprint_covers_efforts_and_couples() {
    let h = Harness::new().await;
    let empty = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    combine_frozen(
        &h,
        EFFORT,
        "m",
        "m",
        &model("high"),
        &[model("")],
        ORIGIN_MANUAL,
    )
    .await
    .unwrap();
    let effort = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    combine_frozen(
        &h,
        COUPLE,
        &couple_scope("p", "n"),
        "",
        &model("m|high"),
        &[model("x|")],
        ORIGIN_MANUAL,
    )
    .await
    .unwrap();
    let couple = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    assert_ne!(empty, effort);
    assert_ne!(effort, couple);
    remove_member(&h, COUPLE, &couple_scope("p", "n"), "x|")
        .await
        .unwrap();
    let back = AbsorptionResolver::from_rows(&h.absorption_rows().await).fingerprint();
    assert_eq!(effort, back);
}

#[tokio::test]
async fn an_absorption_survives_a_reopened_database() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("pdo.db").display());
    {
        let db = SqlitePool::connect(&url).await.unwrap();
        init(&db).await.unwrap();
        SqliteAbsorptions::new(db.clone())
            .combine(
                Scope {
                    dimension: PIPELINE,
                    key: "",
                    name: "",
                },
                &named("b"),
                &[named("a")],
                ORIGIN_MANUAL,
            )
            .await
            .unwrap();
        db.close().await;
    }
    let db = SqlitePool::connect(&url).await.unwrap();
    init(&db).await.unwrap();
    let rows = SqliteAbsorptions::new(db.clone()).list().await.unwrap();
    assert_eq!(
        rows.into_iter()
            .map(|row| (row.member, row.absorbent))
            .collect::<Vec<_>>(),
        vec![edge("a", "b")]
    );
    db.close().await;
}
