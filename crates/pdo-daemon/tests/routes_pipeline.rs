//! Layer 4 (ADR-0080) — the pipeline routes no integration test called before the
//! refactor net (#914): create, rename, delete, the workflow import, the node
//! library delete, and the portable document of a Run's pipeline. Real daemon,
//! real files under `<repo>/.pdo/pipelines/`.

use crate::common::flow;
use crate::common::TestDaemon;

const SOLO: &str = "solo";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[(SOLO, false)])
}

async fn create_pipeline(daemon: &TestDaemon, name: &str) -> (u16, serde_json::Value) {
    flow::post_json(daemon, "/pipelines", serde_json::json!({ "name": name })).await
}

async fn rename(daemon: &TestDaemon, id: &str, name: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .put(format!("{}/pipelines/{id}/rename", daemon.url()))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)),
    )
}

async fn get_pipeline_status(daemon: &TestDaemon, id: &str) -> u16 {
    reqwest::get(format!("{}/pipelines/{id}", daemon.url()))
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn create_pipeline_scaffolds_a_file_named_after_the_slug_and_refuses_duplicates() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = create_pipeline(&daemon, "Bugfix Auto Clean").await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["id"], "bugfix-auto-clean");
    assert_eq!(body["scope"], "instance");
    let path = daemon
        .repo_root()
        .join(".pdo/pipelines/bugfix-auto-clean.yaml");
    assert_eq!(body["path"], path.to_string_lossy().as_ref());
    let yaml = std::fs::read_to_string(&path).unwrap();
    assert!(yaml.starts_with("name: Bugfix Auto Clean\n"), "{yaml}");
    assert_eq!(get_pipeline_status(&daemon, "bugfix-auto-clean").await, 200);

    let (status, body) = create_pipeline(&daemon, "Bugfix Auto Clean").await;
    assert_eq!(status, 409, "same visible name: {body}");
    assert_eq!(body["error"], "pipeline already exists");

    let (status, body) = create_pipeline(&daemon, "   ").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "name is required");
}

#[tokio::test]
async fn rename_moves_the_file_to_the_new_slug_and_the_list_follows() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, _) = create_pipeline(&daemon, "Draft One").await;
    assert_eq!(status, 201);

    let (status, body) = rename(&daemon, "draft-one", "Final Name").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["id"], "final-name");
    assert_eq!(body["name"], "Final Name");
    assert_eq!(body["renamed"], true);

    assert_eq!(get_pipeline_status(&daemon, "final-name").await, 200);
    assert_eq!(get_pipeline_status(&daemon, "draft-one").await, 404);
    let list = daemon.get_json("/pipelines").await;
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert!(names.contains(&"Final Name"), "{list}");
    assert!(!names.contains(&"Draft One"), "{list}");
    let yaml =
        std::fs::read_to_string(daemon.repo_root().join(".pdo/pipelines/final-name.yaml")).unwrap();
    assert!(yaml.starts_with("name: Final Name\n"), "{yaml}");
}

#[tokio::test]
async fn rename_to_the_same_name_is_a_no_op_and_a_display_only_rename_keeps_the_file() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    create_pipeline(&daemon, "Keep Me").await;

    let (status, body) = rename(&daemon, "keep-me", "Keep Me").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["renamed"], false);
    assert_eq!(body["id"], "keep-me");

    // Same slug, different visible name: the `name:` line changes in place.
    let (status, body) = rename(&daemon, "keep-me", "keep me").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["renamed"], true);
    assert_eq!(body["id"], "keep-me");
    let yaml =
        std::fs::read_to_string(daemon.repo_root().join(".pdo/pipelines/keep-me.yaml")).unwrap();
    assert!(yaml.starts_with("name: keep me\n"), "{yaml}");
}

#[tokio::test]
async fn rename_refuses_a_blank_name_an_unknown_pipeline_a_collision_and_an_active_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    create_pipeline(&daemon, "Alpha").await;
    create_pipeline(&daemon, "Beta").await;

    let (status, body) = rename(&daemon, "alpha", "  ").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "name is required");

    let (status, _) = rename(&daemon, "no-such-pipeline", "Gamma").await;
    assert_eq!(status, 404);

    let (status, body) = rename(&daemon, "alpha", "Beta").await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Cannot rename:"),
        "{body}"
    );

    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    let (status, body) = rename(&daemon, SOLO, "Solo Renamed").await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("1 active run(s) reference this pipeline"),
        "{body}"
    );
}

#[tokio::test]
async fn delete_pipeline_removes_the_file_and_its_prompts_but_not_under_an_active_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    create_pipeline(&daemon, "Doomed").await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{}/pipelines/doomed", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({ "ok": true })
    );
    assert!(!daemon
        .repo_root()
        .join(".pdo/pipelines/doomed.yaml")
        .exists());
    assert_eq!(get_pipeline_status(&daemon, "doomed").await, 404);

    let resp = client
        .delete(format!("{}/pipelines/doomed", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "already gone");

    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    let resp = client
        .delete(format!("{}/pipelines/{SOLO}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Cannot delete: 1 active run(s)");
    assert!(daemon
        .repo_root()
        .join(format!(".pdo/pipelines/{SOLO}.yaml"))
        .exists());
    assert!(daemon
        .repo_root()
        .join(format!(".pdo/pipelines/{SOLO}.prompts/worker.md"))
        .exists());
}

#[tokio::test]
async fn library_import_turns_a_workflow_js_into_an_instance_pipeline() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = flow::post_json(
        &daemon,
        "/library/import",
        serde_json::json!({
            "filename": "triage-flow.js",
            "content": "agent(`Triage the incoming bug exactly.`, { label: 'triager' })",
        }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["scope"], "instance");
    let id = body["id"].as_str().unwrap().to_string();
    assert!(body["warnings"].is_array(), "{body}");
    let path = std::path::PathBuf::from(body["path"].as_str().unwrap());
    assert!(path.starts_with(daemon.repo_root().join(".pdo/pipelines")));
    assert!(path.exists(), "{}", path.display());

    let pipeline = daemon.get_json(&format!("/pipelines/{id}")).await;
    assert!(
        pipeline.to_string().contains("triager"),
        "the labelled agent is a node of the imported pipeline: {pipeline}"
    );

    // A second import of the same file does not overwrite the first.
    let (status, again) = flow::post_json(
        &daemon,
        "/library/import",
        serde_json::json!({
            "filename": "triage-flow.js",
            "content": "agent(`Triage the incoming bug exactly.`, { label: 'triager' })",
        }),
    )
    .await;
    assert_eq!(status, 201, "{again}");
    assert_ne!(again["id"], body["id"], "{again}");
}

#[tokio::test]
async fn library_import_refuses_source_it_cannot_parse() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, body) = flow::post_json(
        &daemon,
        "/library/import",
        serde_json::json!({ "filename": "broken.js", "content": "agent(`unterminated" }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("import failed:"),
        "{body}"
    );
}

/// The node library lives under the real `$HOME` (`~/.pdo/library`): the entry is
/// named uniquely and deleted by the test itself.
#[tokio::test]
async fn library_delete_removes_an_entry_and_answers_404_once_gone() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let name = format!("net-914-{}", uuid::Uuid::new_v4().simple());
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/library", daemon.url()))
        .json(&serde_json::json!({
            "name": name,
            "type": "agent",
            "inputs": [],
            "outputs": [],
            "prompt": "Throwaway entry of the route net.",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client
        .delete(format!("{}/library/{name}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    let entries: Vec<serde_json::Value> = reqwest::get(format!("{}/library", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(entries.iter().all(|e| e["name"] != name.as_str()));

    let resp = client
        .delete(format!("{}/library/{name}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn run_pipeline_document_exports_the_runs_snapshot_as_portable_yaml() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;

    let resp = reqwest::get(format!("{}/runs/{run_id}/pipeline/document", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["content-type"],
        "application/yaml; charset=utf-8"
    );
    let document = resp.text().await.unwrap();
    assert!(document.contains("name: solo"), "{document}");
    assert!(
        document.contains("Do the work."),
        "the prompt travels inside the portable document: {document}"
    );

    let resp = reqwest::get(format!(
        "{}/runs/no-such-run/pipeline/document",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}
