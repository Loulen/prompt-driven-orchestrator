//! Layer 3a — proves issue #28: run-scoped pipeline copy and pipeline_modified
//! events. At `RunStarted` the daemon copies the source pipeline YAML and
//! prompts into `<run-id>/pipeline.yaml`. The watcher emits `pipeline_modified`
//! events when the run-scoped copy is edited externally.

mod common;

use std::process::Command;
use std::time::Duration;

use common::{ws_text, TestDaemon};
use futures_util::StreamExt;
use tokio::time::timeout;

const PIPELINE_NAME: &str = "run-edit-test";
const PIPELINE_YAML: &str = r#"name: run-edit-test
version: "1.0"
nodes:
  - id: planner
    type: doc-only
    prompt_file: run-edit-test.prompts/planner.md
    inputs:
      - name: task
    outputs:
      - name: plan
    view: { x: 100, y: 100 }
edges: []
"#;

const PROMPT_CONTENT: &str = "You are a planner. Plan the task.\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("planner.md"), PROMPT_CONTENT)?;
    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
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

/// Wait for a `pipeline_modified` event on the WebSocket for a given run_id.
async fn next_pipeline_modified_event(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    run_id: &str,
    deadline: Duration,
) -> Option<serde_json::Value> {
    let result = timeout(deadline, async {
        loop {
            let next = ws.next().await?;
            let msg = next.ok()?;
            let Some(text) = ws_text(&msg) else {
                continue;
            };
            let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
            if parsed["type"] == "event" {
                if let Some(event) = parsed.get("event") {
                    if event["kind"] == "pipeline_modified" && event["run_id"] == run_id {
                        return Some(event.clone());
                    }
                }
            }
        }
    })
    .await;
    result.ok().flatten()
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
async fn run_creates_pipeline_copy() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // Assert pipeline.yaml was copied to the run dir
    let yaml_path = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("pipeline.yaml");
    assert!(yaml_path.exists(), "run-scoped pipeline.yaml must exist");
    let content = std::fs::read_to_string(&yaml_path).unwrap();
    assert!(
        content.contains("run-edit-test"),
        "content should match source pipeline"
    );

    // Assert prompts dir was copied
    let prompts_dir = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("pipeline.prompts");
    assert!(prompts_dir.is_dir(), "pipeline.prompts dir must exist");
    let prompt = std::fs::read_to_string(prompts_dir.join("planner.md")).unwrap();
    assert_eq!(prompt, PROMPT_CONTENT);
}

#[tokio::test]
async fn external_write_to_run_pipeline_emits_pipeline_modified() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let mut ws = daemon.connect_ws().await.unwrap();
    // Drain initial ready + any run events
    let _ = timeout(Duration::from_millis(500), ws.next()).await;

    // Write directly to the run-scoped pipeline YAML (simulating external edit)
    let yaml_path = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("pipeline.yaml");

    let updated_yaml = PIPELINE_YAML.replace("version: \"1.0\"", "version: \"2.0\"");
    std::fs::write(&yaml_path, updated_yaml).unwrap();

    // Should receive a pipeline_modified event within the debounce window.
    // The watcher may also have picked up the initial prompt copy, so we look
    // for any pipeline_modified event for this run_id.
    let evt = next_pipeline_modified_event(&mut ws, &run_id, Duration::from_secs(4))
        .await
        .expect("external write to run-scoped pipeline should emit pipeline_modified within 4s");

    assert_eq!(evt["kind"], "pipeline_modified");
    assert_eq!(evt["run_id"], run_id);
}

#[tokio::test]
async fn cleanup_run_removes_pipeline_copy() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let run_dir = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id);
    assert!(run_dir.join("pipeline.yaml").exists());

    // Cleanup
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "cleanup should succeed");

    // The entire run dir (including pipeline.yaml) should be gone
    assert!(!run_dir.exists(), "run dir should be removed after cleanup");
}

/// Wait for a specific event kind + node_id on the WebSocket.
async fn next_event_for_node(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    kind: &str,
    node_id: &str,
    deadline: Duration,
) -> Option<serde_json::Value> {
    let result = timeout(deadline, async {
        loop {
            let next = ws.next().await?;
            let msg = next.ok()?;
            let Some(text) = ws_text(&msg) else {
                continue;
            };
            let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
            if parsed["type"] == "event" {
                if let Some(event) = parsed.get("event") {
                    if event["kind"] == kind && event["node_id"] == node_id {
                        return Some(event.clone());
                    }
                }
            }
        }
    })
    .await;
    result.ok().flatten()
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn adding_node_to_run_pipeline_triggers_scheduler_spawn() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // Use sleep as a substitute for claude
    std::env::set_var("MAESTRO_TMUX_CMD_OVERRIDE", "exec sleep 300");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let mut ws = daemon.connect_ws().await.unwrap();
    // Drain initial messages (ready + node_started for planner)
    let _ = timeout(Duration::from_secs(2), async {
        loop {
            if ws.next().await.is_none() {
                break;
            }
        }
    })
    .await;

    // Complete the planner node so the new downstream node's inputs are satisfied
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({
            "kind": "mark_node_done",
            "node_id": "planner",
            "iter": 1
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "mark_node_done should succeed");

    // Drain the node_completed and any intermediate events
    let _ = timeout(Duration::from_secs(1), async {
        loop {
            if ws.next().await.is_none() {
                break;
            }
        }
    })
    .await;

    // Now add a downstream node whose upstream (planner) is already completed.
    // The watcher should detect the change, emit pipeline_modified, and the
    // scheduler should spawn the new node.
    let yaml_path = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("pipeline.yaml");

    let new_yaml = r#"name: run-edit-test
version: "1.0"
nodes:
  - id: planner
    type: doc-only
    prompt_file: run-edit-test.prompts/planner.md
    inputs:
      - name: task
    outputs:
      - name: plan
    view: { x: 100, y: 100 }
  - id: implementer
    type: doc-only
    inputs:
      - name: plan
    outputs:
      - name: summary
    view: { x: 300, y: 100 }
edges:
  - source: { node: planner, port: plan }
    target: { node: implementer, port: plan }
"#;
    std::fs::write(&yaml_path, new_yaml).unwrap();

    // Should see a node_started event for implementer within the debounce + scheduler eval window
    let evt = next_event_for_node(
        &mut ws,
        "node_started",
        "implementer",
        Duration::from_secs(6),
    )
    .await;

    // Kill any tmux sessions we spawned
    let _ = Command::new("tmux").args(["kill-server"]).output();
    std::env::remove_var("MAESTRO_TMUX_CMD_OVERRIDE");

    assert!(
        evt.is_some(),
        "scheduler should spawn the new 'implementer' node after pipeline_modified"
    );
}

#[tokio::test]
async fn get_run_returns_augmented_node_defs() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // Modify the run-scoped YAML to add a node
    let yaml_path = daemon
        .repo_root()
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("pipeline.yaml");

    let new_yaml = r#"name: run-edit-test
version: "1.0"
nodes:
  - id: planner
    type: doc-only
    prompt_file: run-edit-test.prompts/planner.md
    inputs:
      - name: task
    outputs:
      - name: plan
    view: { x: 100, y: 100 }
  - id: implementer
    type: code-mutating
    inputs:
      - name: plan
    outputs:
      - name: summary
    view: { x: 300, y: 100 }
edges:
  - source: { node: planner, port: plan }
    target: { node: implementer, port: plan }
"#;
    std::fs::write(&yaml_path, new_yaml).unwrap();

    // GET /runs/:id should reflect the updated node_defs
    let resp = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run_state: serde_json::Value = resp.json().await.unwrap();
    let node_defs = run_state["node_defs"].as_array().unwrap();
    assert_eq!(node_defs.len(), 2, "should reflect augmented node_defs");
    let ids: Vec<&str> = node_defs
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"planner"));
    assert!(ids.contains(&"implementer"));

    let edges = run_state["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1, "should reflect augmented edges");
}
