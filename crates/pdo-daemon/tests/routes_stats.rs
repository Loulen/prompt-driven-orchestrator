//! Layer 4 (ADR-0080) — the Stats routes no integration test called before the
//! refactor net (#914): `GET /stats/performance` and the absorptions (#888,
//! ADR-0077) — list, combine, uncombine a member — read back through the
//! performance fold.
//!
//! The daemon's home is the tempdir, so no transcript of the real `~/.claude`
//! leaks into the fold.

use std::path::Path;

use crate::common::flow;
use crate::common::TestDaemon;

const WINDOW: &str = "from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z";

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("perf-a", false), ("perf-b", false)])
}

async fn spawn() -> TestDaemon {
    TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap()
}

async fn performance(daemon: &TestDaemon, extra: &str) -> serde_json::Value {
    daemon
        .get_json(&format!("/stats/performance?{WINDOW}&refresh=true{extra}"))
        .await
}

fn pipeline_ids(perf: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = perf["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

async fn combine_pipelines(daemon: &TestDaemon) -> (u16, serde_json::Value) {
    flow::post_json(
        daemon,
        "/stats/absorptions",
        serde_json::json!({
            "dimension": "pipeline",
            "absorbent": { "key": "perf-a", "name": "perf-a" },
            "members": [{ "key": "perf-b", "name": "perf-b" }],
        }),
    )
    .await
}

async fn uncombine(daemon: &TestDaemon, query: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .delete(format!("{}/stats/absorptions/member?{query}", daemon.url()))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

#[tokio::test]
async fn performance_is_empty_on_a_fresh_instance() {
    let daemon = spawn().await;
    let perf = performance(&daemon, "").await;
    assert_eq!(perf["harnesses"], serde_json::json!([]), "{perf}");
    assert_eq!(perf["by_pipeline"], serde_json::json!([]));
    assert_eq!(perf["by_model"], serde_json::json!([]));
    assert_eq!(perf["executions"], 0);
    assert_eq!(perf["waited_executions"], 0);
    assert_eq!(perf["total"]["harnesses"], serde_json::json!([]));
}

#[tokio::test]
async fn performance_folds_one_row_per_pipeline_and_honours_the_cohort_switch() {
    let daemon = spawn().await;
    flow::completed_solo_run(&daemon, "perf-a").await;
    flow::completed_solo_run(&daemon, "perf-b").await;
    let live = flow::create_run(&daemon, "perf-b").await;
    flow::wait_node_status(&daemon, &live, "worker", "running").await;

    let perf = performance(&daemon, "").await;
    assert_eq!(pipeline_ids(&perf), vec!["perf-a", "perf-b"], "{perf}");
    assert_eq!(
        perf["executions"], 2,
        "one successful worker per completed run"
    );
    let row_b = perf["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "perf-b")
        .unwrap();
    assert_eq!(
        row_b["runs"], 2,
        "the live run counts in the cohort: {row_b}"
    );
    assert!(
        row_b["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "worker"),
        "{row_b}"
    );

    let completed = performance(&daemon, "&completed_only=true").await;
    let row_b = completed["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "perf-b")
        .unwrap();
    assert_eq!(row_b["runs"], 1, "completed runs only: {row_b}");

    let resp = reqwest::get(format!("{}/stats/performance", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "the window is required");
}

#[tokio::test]
async fn combining_two_pipelines_folds_them_into_one_row_until_uncombined() {
    let daemon = spawn().await;
    flow::completed_solo_run(&daemon, "perf-a").await;
    flow::completed_solo_run(&daemon, "perf-b").await;
    assert_eq!(
        daemon.get_json("/stats/absorptions").await,
        serde_json::json!({ "absorptions": [] })
    );

    let (status, list) = combine_pipelines(&daemon).await;
    assert_eq!(status, 200, "{list}");
    let absorption = &list["absorptions"][0];
    assert_eq!(absorption["dimension"], "pipeline");
    assert_eq!(absorption["scope"], "");
    assert_eq!(
        absorption["absorbent"],
        serde_json::json!({ "key": "perf-a", "name": "perf-a" })
    );
    assert_eq!(absorption["members"][0]["key"], "perf-b");
    assert_eq!(absorption["members"][0]["origin"], "manual");
    assert_eq!(daemon.get_json("/stats/absorptions").await, list);

    let combined = performance(&daemon, "").await;
    assert_eq!(pipeline_ids(&combined), vec!["perf-a"], "{combined}");
    assert_eq!(combined["by_pipeline"][0]["runs"], 2, "{combined}");
    let raw = performance(&daemon, "&uncombined=true").await;
    assert_eq!(pipeline_ids(&raw), vec!["perf-a", "perf-b"], "{raw}");

    let audit = daemon.get_json("/audit").await;
    assert!(
        audit.to_string().contains("stats_absorption.combine"),
        "{audit}"
    );

    let (status, list) = uncombine(&daemon, "dimension=pipeline&member=perf-b").await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list, serde_json::json!({ "absorptions": [] }));
    let split = performance(&daemon, "").await;
    assert_eq!(pipeline_ids(&split), vec!["perf-a", "perf-b"], "{split}");

    let (status, body) = uncombine(&daemon, "dimension=pipeline&member=perf-b").await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "this row is not combined");
}

#[tokio::test]
async fn absorptions_refuse_what_stats_cannot_combine() {
    let daemon = spawn().await;

    for (request, error) in [
        (
            serde_json::json!({
                "dimension": "harness",
                "absorbent": { "key": "a", "name": "a" },
                "members": [{ "key": "b", "name": "b" }],
            }),
            "unsupported absorption dimension: harness",
        ),
        (
            serde_json::json!({
                "dimension": "pipeline",
                "absorbent": { "key": "a", "name": "a" },
                "members": [{ "key": "a", "name": "a" }],
            }),
            "an absorption needs at least one other member",
        ),
        (
            serde_json::json!({
                "dimension": "pipeline",
                "scope": "x",
                "absorbent": { "key": "a", "name": "a" },
                "members": [{ "key": "b", "name": "b" }],
            }),
            "a pipeline absorption has no scope",
        ),
        (
            serde_json::json!({
                "dimension": "node",
                "absorbent": { "key": "a", "name": "a" },
                "members": [{ "key": "b", "name": "b" }],
            }),
            "a node absorption needs the pipeline its nodes run under",
        ),
    ] {
        let (status, body) = flow::post_json(&daemon, "/stats/absorptions", request).await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"], error);
    }

    let (status, body) = uncombine(&daemon, "dimension=harness&member=x").await;
    assert_eq!(status, 400, "{body}");
}
