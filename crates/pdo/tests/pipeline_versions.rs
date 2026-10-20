//! Layer 4 (ADR-0080) — the versions of a pipeline (ADR-0088) over HTTP: a
//! Save of the draft freezes the next version, or replaces the last one, or
//! keeps it as a pipeline of its own; the history lists them; a Run retains
//! the version it launched, which the history counts.

use serde_json::{json, Value};

use crate::common::flow::{create_run, seed_solo, solo_pipeline_yaml};
use crate::common::TestDaemon;

async fn put(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .put(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Save the draft of `id` as `yaml`, as the « Save… » window says.
async fn save(daemon: &TestDaemon, id: &str, yaml: &str, version: Value) -> Value {
    let (status, body) = put(
        daemon,
        &format!("/pipelines/{id}"),
        json!({ "yaml": yaml, "prompts": {}, "draft": { "pipeline": {}, "prompts": {} }, "version": version }),
    )
    .await;
    assert_eq!(status, 200, "save: {body}");
    body
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("versioned", false)])
}

#[tokio::test]
async fn a_save_numbers_the_versions_and_a_run_retains_the_one_it_launched() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Before any Save, the file is v1, titled by its free « Version » field.
    let listed = daemon.get_json("/pipelines").await;
    let entry = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "versioned")
        .unwrap();
    assert_eq!(entry["version"], json!({ "number": 1, "title": "1.0" }));

    let edited = solo_pipeline_yaml("versioned", true);
    let saved = save(
        &daemon,
        "versioned",
        &edited,
        json!({ "new_version": true, "title": "Gate the worker" }),
    )
    .await;
    assert_eq!(
        saved["version"],
        json!({ "number": 2, "title": "Gate the worker" })
    );

    let run_id = create_run(&daemon, "versioned").await;
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(
        run["pipeline_version"],
        json!({ "number": 2, "title": "Gate the worker" })
    );

    let history = daemon.get_json("/pipelines/versioned/versions").await;
    let versions = history["versions"].as_array().unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|v| (v["number"].clone(), v["runs"].clone()))
            .collect::<Vec<_>>(),
        [(json!(2), json!(1)), (json!(1), json!(0))]
    );
    assert_eq!(versions[1]["author"], Value::Null);
    assert_eq!(versions[0]["author"]["kind"], "local_operator");

    // One version reads back as the editor shows it.
    let first = daemon.get_json("/pipelines/versioned/versions/1").await;
    assert_eq!(first["title"], "1.0");
    assert_eq!(first["pipeline"]["nodes"][1]["interactive"], false);
}

#[tokio::test]
async fn a_replacement_keeps_the_number_and_keep_old_lists_a_new_pipeline() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let edited = solo_pipeline_yaml("versioned", true);
    let saved = save(
        &daemon,
        "versioned",
        &edited,
        json!({ "new_version": false, "keep_old": true }),
    )
    .await;
    assert_eq!(saved["version"]["number"], 1);
    let kept = saved["kept"]["id"].as_str().unwrap().to_string();

    let listed = daemon.get_json("/pipelines").await;
    let entry = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == kept.as_str())
        .unwrap();
    assert_eq!(entry["name"], "versioned (v1)");
    let original = daemon.get_json(&format!("/pipelines/{kept}")).await;
    assert_eq!(original["pipeline"]["nodes"][1]["interactive"], false);
    let history = daemon.get_json("/pipelines/versioned/versions").await;
    assert_eq!(history["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_pipeline_or_version_is_not_found() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    for path in [
        "/pipelines/nope/versions",
        "/pipelines/versioned/versions/9",
    ] {
        let status = reqwest::get(format!("{}{path}", daemon.url()))
            .await
            .unwrap()
            .status();
        assert_eq!(status, 404, "{path}");
    }
}
