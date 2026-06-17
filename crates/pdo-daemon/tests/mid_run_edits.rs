//! Layer 3a — #211 / #206: mid-run edit policy through the HTTP boundary
//! (ADR-0007 enforcement). Dangerous edits are rejected with an explicit
//! message; safe edits keep working exactly as before:
//!   - changing the type of a node with a live session → 409 with reason
//!   - adding a node + edge during a run → 200
//!   - editing the prompt of a not-yet-spawned node → 200, prompt persisted

mod common;

use std::time::Duration;

use common::TestDaemon;

const PIPELINE_NAME: &str = "mid-run-edit-test";
const PIPELINE_YAML: &str = r#"name: mid-run-edit-test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are a worker.\n")?;
    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "test@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "variables": {}
    });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

/// Poll the run projection until `node_id` reaches one of `statuses` (the
/// scheduler spawns asynchronously after RunStarted).
async fn wait_for_node_status(
    daemon_url: &str,
    run_id: &str,
    node_id: &str,
    statuses: &[&str],
) -> String {
    for _ in 0..100 {
        let resp = reqwest::Client::new()
            .get(format!("{daemon_url}/runs/{run_id}"))
            .send()
            .await
            .unwrap();
        if resp.status() == 200 {
            let json: serde_json::Value = resp.json().await.unwrap();
            if let Some(status) = json["nodes"][node_id]["status"].as_str() {
                if statuses.contains(&status) {
                    return status.to_string();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("node '{node_id}' never reached one of {statuses:?}");
}

#[tokio::test]
async fn changing_type_of_live_node_is_rejected_with_message() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;
    wait_for_node_status(&daemon.url(), &run_id, "worker", &["running"]).await;

    // Same graph, but the worker's type flips doc-only -> code-mutating.
    let yaml_with_type_change = PIPELINE_YAML.replace(
        "    name: Worker\n    type: doc-only",
        "    name: Worker\n    type: code-mutating",
    );
    assert!(yaml_with_type_change.contains("code-mutating"), "guard");

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({ "yaml": yaml_with_type_change, "prompts": {} }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        409,
        "changing the type of a running node must be rejected"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "mutation rejected");
    let rejections = body["rejections"].as_array().unwrap();
    assert_eq!(rejections[0]["node_id"], "worker");
    let reason = rejections[0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("type"),
        "rejection must explain the type immutability; got: {reason}"
    );
}

#[tokio::test]
async fn adding_node_and_edge_mid_run_succeeds() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;
    wait_for_node_status(&daemon.url(), &run_id, "worker", &["running"]).await;

    // ADR-0007 (c): free addition of node + edge while the run is live.
    let yaml_with_reviewer = r#"name: mid-run-edit-test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: reviewer
    name: Reviewer
    type: doc-only
    outputs:
      - name: feedback
    view: { x: 400, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: reviewer, port: code }
"#;

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({ "yaml": yaml_with_reviewer, "prompts": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "adding a node + edge during a run must stay allowed"
    );
}

#[tokio::test]
async fn prompt_edit_of_unspawned_node_succeeds() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;
    wait_for_node_status(&daemon.url(), &run_id, "worker", &["running"]).await;

    // Add a pending reviewer (no incoming edge satisfied yet) with a prompt.
    let yaml_with_reviewer = r#"name: mid-run-edit-test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: reviewer
    name: Reviewer
    type: doc-only
    outputs:
      - name: feedback
    view: { x: 400, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: reviewer, port: code }
"#;

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({
            "yaml": yaml_with_reviewer,
            "prompts": { "reviewer": "Review the diff carefully.\n" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "editing the prompt of an unspawned node must stay allowed"
    );

    // The run-scoped prompt must be persisted and visible on read-back.
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["prompts"]["reviewer"], "Review the diff carefully.\n",
        "run-scoped prompt edit must round-trip"
    );
}
