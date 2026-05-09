//! Layer 3a — proves issue #57 mutation policy: save_run_pipeline rejects
//! illegal mutations (deleting non-pending nodes) with 409 and auto-syncs
//! valid edits to the library template via atomic tmp+rename.

mod common;

use common::TestDaemon;
use std::process::Command;

const PIPELINE_NAME: &str = "mutation-test";
const PIPELINE_YAML: &str = r#"name: mutation-test
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
    let pipelines_dir = repo.join(".maestro").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are a worker.\n")?;
    git_init(repo)?;
    Ok(())
}

fn git_init(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-b", "main"])?;
    run(&["config", "user.email", "test@test.com"])?;
    run(&["config", "user.name", "Test"])?;
    std::fs::write(repo.join("README.md"), "test")?;
    run(&["add", "."])?;
    run(&["commit", "-m", "init"])?;
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

#[tokio::test]
async fn delete_running_node_returns_409() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // Worker should be running (started by scheduler).
    // Try to save a pipeline YAML that deletes the worker node.
    let yaml_without_worker = r#"name: mutation-test
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
"#;

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({
            "yaml": yaml_without_worker,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        409,
        "deleting a running node should be rejected with 409"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "mutation rejected");
    let rejections = body["rejections"].as_array().unwrap();
    assert!(!rejections.is_empty());
    assert_eq!(rejections[0]["node_id"], "worker");
}

#[tokio::test]
async fn add_node_succeeds_and_syncs_to_template() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let yaml_with_new_node = r#"name: mutation-test
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
    inputs:
      - name: code
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
"#;

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({
            "yaml": yaml_with_new_node,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "adding a new node should succeed");

    // Verify auto-sync: template file should now contain the reviewer node
    let template_path = daemon
        .repo_root()
        .join(".maestro")
        .join("pipelines")
        .join(format!("{PIPELINE_NAME}.yaml"));
    let template_content = std::fs::read_to_string(&template_path).unwrap();
    assert!(
        template_content.contains("reviewer"),
        "template should be updated with auto-synced content"
    );
}

#[tokio::test]
async fn delete_pending_node_succeeds() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // Add a new pending node first
    let yaml_with_extra = r#"name: mutation-test
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
  - id: pending-node
    name: PendingNode
    type: doc-only
    inputs:
      - name: data
    outputs:
      - name: output
    view: { x: 400, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

    // First add the pending node
    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({
            "yaml": yaml_with_extra,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "adding pending node should succeed");

    // Now remove the pending node — should succeed since it was never started
    let yaml_without_pending = r#"name: mutation-test
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

    let resp = reqwest::Client::new()
        .put(format!("{}/runs/{}/pipeline", daemon.url(), run_id))
        .json(&serde_json::json!({
            "yaml": yaml_without_pending,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "deleting a pending (never-started) node should succeed"
    );
}
