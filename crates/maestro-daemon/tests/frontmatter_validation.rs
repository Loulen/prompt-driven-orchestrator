//! Layer 3a — frontmatter validation + retry integration test for issue #59.
//!
//! Spawns a real TestDaemon, creates a run with a node that has a frontmatter
//! schema on its output port. Tests:
//!   1. Valid frontmatter → NodeCompleted
//!   2. Invalid frontmatter (1st attempt) → frontmatter_retry_pending, node stays Running
//!   3. Invalid frontmatter (2nd attempt) → NodeFailed with "output validation failed"
//!   4. Invalid then valid → NodeCompleted on retry

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "fm-validate";
const PIPELINE_YAML: &str = r#"name: fm-validate
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
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
    std::fs::write(prompts_dir.join("reviewer.md"), "You are a reviewer.\n")?;
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
    run(&["init"])?;
    run(&["config", "user.email", "test@test.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["add", "."])?;
    run(&["commit", "-m", "init"])?;
    Ok(())
}

fn write_artifact(
    repo: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    port_name: &str,
    content: &str,
) {
    let dir = repo
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("worktree/.maestro/artifacts")
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join(port_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), content).unwrap();
}

async fn create_run(daemon: &TestDaemon) -> String {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "test input",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resp["run_id"].as_str().unwrap().to_string()
}

async fn get_run_state(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    let client = reqwest::Client::new();
    client
        .get(format!("{}/runs/{run_id}", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn mark_node_done(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "mark_node_done",
            "node_id": node_id,
            "iter": iter,
        }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn valid_frontmatter_completes_node() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    write_artifact(
        daemon.repo_root(),
        &run_id,
        "reviewer",
        1,
        "review",
        "---\nverdict: PASS\nscore: 8\n---\nLGTM",
    );

    let resp = mark_node_done(&daemon, &run_id, "reviewer", 1).await;
    assert!(resp.status().is_success());

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let state = get_run_state(&daemon, &run_id).await;
    let reviewer = &state["nodes"]["reviewer"];
    assert_eq!(reviewer["status"], "completed");
}

#[tokio::test]
async fn invalid_frontmatter_triggers_retry_then_valid_succeeds() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // First attempt: invalid enum value
    write_artifact(
        daemon.repo_root(),
        &run_id,
        "reviewer",
        1,
        "review",
        "---\nverdict: MAYBE\nscore: 8\n---\nNot sure",
    );

    let resp = mark_node_done(&daemon, &run_id, "reviewer", 1).await;
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "frontmatter_retry_pending");

    // Check node is still running with retries > 0
    let state = get_run_state(&daemon, &run_id).await;
    let reviewer = &state["nodes"]["reviewer"];
    assert_eq!(reviewer["status"], "running");
    assert_eq!(reviewer["frontmatter_retries"], 1);

    // Second attempt: valid frontmatter
    write_artifact(
        daemon.repo_root(),
        &run_id,
        "reviewer",
        1,
        "review",
        "---\nverdict: PASS\nscore: 9\n---\nLGTM after fix",
    );

    let resp2 = mark_node_done(&daemon, &run_id, "reviewer", 1).await;
    assert!(resp2.status().is_success());

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let state2 = get_run_state(&daemon, &run_id).await;
    let reviewer2 = &state2["nodes"]["reviewer"];
    assert_eq!(reviewer2["status"], "completed");
}

#[tokio::test]
async fn double_invalid_frontmatter_fails_node() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // First attempt: invalid
    write_artifact(
        daemon.repo_root(),
        &run_id,
        "reviewer",
        1,
        "review",
        "---\nverdict: MAYBE\nscore: 8\n---\nNot sure",
    );

    let resp1 = mark_node_done(&daemon, &run_id, "reviewer", 1).await;
    let body1: serde_json::Value = resp1.json().await.unwrap();
    assert_eq!(body1["status"], "frontmatter_retry_pending");

    // Second attempt: still invalid
    write_artifact(
        daemon.repo_root(),
        &run_id,
        "reviewer",
        1,
        "review",
        "---\nverdict: MAYBE\nscore: 8\n---\nStill not sure",
    );

    let resp2 = mark_node_done(&daemon, &run_id, "reviewer", 1).await;
    assert!(resp2.status().is_success());
    let body2: serde_json::Value = resp2.json().await.unwrap();
    assert_eq!(body2["status"], "frontmatter_retry_exhausted");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let state = get_run_state(&daemon, &run_id).await;
    let reviewer = &state["nodes"]["reviewer"];
    assert_eq!(reviewer["status"], "failed");
    assert_eq!(reviewer["failure_reason"], "output validation failed");
    let violations = reviewer["frontmatter_violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["field"], "verdict");
    assert!(violations[0]["reason"].as_str().unwrap().contains("MAYBE"));
}
