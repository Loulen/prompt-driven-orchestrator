//! Layer 3a — start_node projection integration test for issue #30.
//!
//! Spawns a real TestDaemon, creates a run via POST /runs with input "hello world",
//! then asserts that GET /runs/{run_id} returns a RunState with start_node populated
//! and that GET /runs/{run_id}/artifact?path=_input/output.md returns the input content.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "start-node-test";
const PIPELINE_YAML: &str = r#"name: start-node-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: only
    name: only
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: out
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: only, port: task }
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
    std::fs::write(prompts_dir.join("only.md"), "You are a worker.\n")?;

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
    std::fs::write(repo.join(".gitignore"), ".maestro/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "hello world",
    });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn run_state_includes_start_node_with_entry_targets() {
    unsafe {
        std::env::set_var("MAESTRO_TMUX_CMD_OVERRIDE", "exec sleep 300");
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let resp = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let run_state: serde_json::Value = resp.json().await.unwrap();

    let start_node = &run_state["start_node"];
    assert!(!start_node.is_null(), "start_node should be non-null");
    assert_eq!(start_node["input_path"], "_input/output.md");
    assert!(
        start_node["started_at"].as_str().is_some(),
        "started_at should be a string"
    );

    let targets = start_node["target_node_ids"]
        .as_array()
        .expect("target_node_ids should be an array");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0], "only");
}

#[tokio::test]
async fn run_state_includes_end_node_with_pending_port() {
    unsafe {
        std::env::set_var("MAESTRO_TMUX_CMD_OVERRIDE", "exec sleep 300");
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let resp = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let run_state: serde_json::Value = resp.json().await.unwrap();

    let end_node = &run_state["end_node"];
    assert!(!end_node.is_null(), "end_node should be non-null");
    assert_eq!(end_node["id"], "end");

    let ports = end_node["ports"]
        .as_array()
        .expect("ports should be an array");
    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0]["port_name"], "result");
    assert_eq!(ports[0]["status"], "pending");
    assert!(ports[0]["reason"].is_null());
}

#[tokio::test]
async fn artifact_endpoint_serves_input_md() {
    unsafe {
        std::env::set_var("MAESTRO_TMUX_CMD_OVERRIDE", "exec sleep 300");
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let resp = reqwest::get(format!(
        "{}/runs/{}/artifact?path=_input/output.md",
        daemon.url(),
        run_id
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
    let content = resp.text().await.unwrap();
    assert_eq!(content, "hello world");
}
