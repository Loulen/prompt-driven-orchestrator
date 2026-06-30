//! Layer 3a — proves #279 Layer 1 is fixed: when a node spawn aborts *after*
//! the sub-worktree is created but *before* `NodeStarted` is appended, the
//! daemon no longer wedges the run `running` forever with an orphaned worktree
//! and no live node. Instead the spawn window's `catch_unwind` isolation reaps
//! the orphan and fails the run loud (`RunFailed`).
//!
//! The abort is forced deterministically with the `PDO_DEBUG_PANIC_SPAWN`
//! one-shot poison (armed per-daemon via `arm_spawn_panic`, no process-global
//! env — #181). The entry node is `code-mutating` so the spawn creates a
//! sub-worktree to reap; arming the poison before `POST /runs` makes the
//! entry-node spawn (the first and only `spawn_node` call) consume it. No tmux
//! is required: the panic fires before the session spawn.

mod common;

use std::time::Duration;

use common::TestDaemon;

const PIPELINE_NAME: &str = "spawn-abort";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: spawn-abort
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: code-mutating
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
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

fn branch_exists(repo: &std::path::Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .args(["branch", "--list", branch])
        .current_dir(repo)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

async fn run_status(daemon: &TestDaemon, run_id: &str) -> Option<String> {
    let runs: serde_json::Value = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    runs.as_array()?
        .iter()
        .find(|r| r["run_id"].as_str() == Some(run_id))
        .and_then(|r| r["status"].as_str())
        .map(String::from)
}

async fn events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let v: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v.as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn spawn_panic_reaps_orphan_and_fails_run_loud() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.repo_root().to_path_buf();

    // Arm the one-shot spawn poison BEFORE creating the run, so the entry node's
    // spawn (the first and only spawn_node call) consumes it.
    daemon.arm_spawn_panic();

    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "test input" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    // The panic is caught inside spawn_node, so the create-run handler returns
    // normally — a lying success is exactly what we replace with a loud failure.
    assert_eq!(
        resp.status(),
        201,
        "POST /runs should still return 201 — the spawn panic is caught, not propagated"
    );
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The entry spawn runs synchronously in the create-run handler, so the
    // panic has already been caught, the orphan reaped, and RunFailed appended.
    // Poll a short window to absorb any async slack.
    let mut failed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if run_status(&daemon, &run_id).await.as_deref() == Some("failed") {
            failed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        failed,
        "run must be Failed loud after the spawn panic, not wedged Running with no live node"
    );

    // Layer 1's reap: the orphaned sub-worktree dir and its branch are gone.
    let sub_wt = repo
        .join(".pdo")
        .join("runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");
    let sub_branch = format!("pdo/sub-{run_id}-{NODE_ID}-iter-1");
    assert!(
        !sub_wt.exists(),
        "orphaned sub-worktree {} must be reaped after the aborted spawn",
        sub_wt.display()
    );
    assert!(
        !branch_exists(&repo, &sub_branch),
        "orphaned branch {sub_branch} must be deleted after the aborted spawn"
    );

    let evs = events(&daemon, &run_id).await;
    // No NodeStarted for the worker — the spawn aborted before it.
    assert!(
        !evs.iter()
            .any(|e| e["kind"] == "node_started" && e["node_id"] == NODE_ID),
        "no NodeStarted may exist for a spawn that aborted before start"
    );
    // The failure is recorded as RunFailed (NOT NodeFailed, which the transition
    // guard would no-op for a node with no NodeStarted, leaving the run Running).
    assert!(
        evs.iter().any(|e| e["kind"] == "run_failed"),
        "the spawn abort must surface as a RunFailed"
    );
    assert!(
        !evs.iter().any(|e| e["kind"] == "node_failed"),
        "the abort must NOT be a NodeFailed (it would no-op and wedge the run)"
    );
}
