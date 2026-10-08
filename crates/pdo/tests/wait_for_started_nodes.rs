//! Layer 3a — « Attente des nœuds lancés » (ADR-0086) over a real daemon: a
//! Run that reaches `End` while a node it launched still runs stays
//! `running`, says how many nodes it waits for, then ends when the node is
//! done — and the list of Runs follows.

use crate::common::TestDaemon;
use crate::declared_wait::{get_json, wait_node_status};

const PIPELINE: &str = "waiting";

/// `start → worker → end`, plus the on-demand `review` fed by `worker`.
const YAML: &str = r#"name: waiting
version: "1.0"
wait_for_started_nodes: true
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: review
    name: review
    type: agent
    isolated_worktree: false
    on_demand: true
    outputs:
      - name: result
    view: { x: 200, y: 300 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: review, port: work }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(pipelines_dir.join(format!("{PIPELINE}.yaml")), YAML)?;
    let prompts = pipelines_dir.join(format!("{PIPELINE}.prompts"));
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(prompts.join("worker.md"), "You work.\n")?;
    std::fs::write(prompts.join("review.md"), "You review.\n")?;
    let git = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["config", "user.email", "test@example.com"])?;
    git(&["config", "user.name", "Test"])?;
    git(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    git(&["add", "."])?;
    git(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn post(daemon: &TestDaemon, path: &str, body: serde_json::Value) -> (u16, String) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    (
        resp.status().as_u16(),
        resp.text().await.unwrap_or_default(),
    )
}

async fn complete(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    let dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(node_id)
        .join("iter-1/result");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "done\n").unwrap();
    let (status, body) = post(
        daemon,
        &format!("/runs/{run_id}/nodes/{node_id}/done"),
        serde_json::json!({ "iter": 1 }),
    )
    .await;
    assert_eq!(status, 200, "completion of {node_id}: {body}");
}

async fn list_entry(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    let list = get_json(daemon, "/runs").await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == run_id)
        .cloned()
        .unwrap_or_else(|| panic!("run {run_id} missing from GET /runs: {list}"))
}

async fn wait_run_status(daemon: &TestDaemon, run_id: &str, want: &str) -> serde_json::Value {
    for _ in 0..600 {
        let run = get_json(daemon, &format!("/runs/{run_id}")).await;
        if run["status"] == want {
            return run;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("run {run_id} never reached `{want}`");
}

#[tokio::test]
async fn a_run_stays_running_at_end_while_a_started_node_runs_then_ends() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, body) = post(
        &daemon,
        "/runs",
        serde_json::json!({
            "pipeline": PIPELINE,
            "input": "work",
            "name": "waiting-run",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let run_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    // The runtime left the on-demand node alone; the user launches it.
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert!(run["nodes"]["review"].is_null(), "{run}");
    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/nodes/review/start"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    wait_node_status(&daemon, &run_id, "review", "running").await;

    // `End` is reached, but `review` still runs: the Run waits for it.
    complete(&daemon, &run_id, "worker").await;
    wait_node_status(&daemon, &run_id, "worker", "completed").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "running", "{run}");
    assert_eq!(run["waiting_for_started_nodes"], 1, "{run}");
    let entry = list_entry(&daemon, &run_id).await;
    assert_eq!(entry["status"], "running", "{entry}");
    assert_eq!(entry["waiting_for_started_nodes"], 1, "{entry}");

    // The last started node completes: the Run ends, and the list follows.
    complete(&daemon, &run_id, "review").await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert!(run["waiting_for_started_nodes"].is_null(), "{run}");
    let entry = list_entry(&daemon, &run_id).await;
    assert_eq!(entry["status"], "completed", "{entry}");
    assert!(entry.get("waiting_for_started_nodes").is_none(), "{entry}");
}
