//! Layer 3a (#550, ADR-0046) — the harness axis, end to end through the daemon.
//!
//! A two-node `doc-only` pipeline: one node with no pin (⇒ the `claude` floor),
//! one `pin_harness: opencode`. Spawned through the **tmux command seam** (a
//! harmless `sleep`, never a real agent), the test proves:
//!   1. the resolved harness is **frozen** in each node's `NodeStarted` event, and
//!   2. a resume of the pinned node **re-poses** that frozen harness (ADR-0007).
//!
//! The binary fail-fast (AC #10) is deliberately not exercised here: it is
//! skipped under the command override (an override means the real binary never
//! runs), and it is unit-tested in `node_spawn` / `node_primitives`.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "harness-test";
const PIPELINE_YAML: &str = r#"name: harness-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aaaaaaaa
    name: on-claude
    type: doc-only
    outputs:
      - name: out
    view: { x: 200, y: 60 }
  - id: bbbbbbbb
    name: on-opencode
    type: doc-only
    pin_harness: opencode
    outputs:
      - name: out
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aaaaaaaa, port: task }
  - source: { node: start, port: user_prompt }
    target: { node: bbbbbbbb, port: task }
  - source: { node: aaaaaaaa, port: out }
    target: { node: end, port: result }
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
    std::fs::write(prompts_dir.join("aaaaaaaa.md"), "You are on claude.\n")?;
    std::fs::write(prompts_dir.join("bbbbbbbb.md"), "You are on opencode.\n")?;
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

async fn create_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "go",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
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

/// The `harness` frozen on a node's latest `NodeStarted`, or `None` until it lands.
fn started_harness(evs: &[serde_json::Value], node_id: &str) -> Option<String> {
    evs.iter()
        .rev()
        .find(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
        .and_then(|e| e["payload"]["harness"].as_str())
        .map(String::from)
}

/// Poll the event log until both entry nodes have a `NodeStarted`, or time out.
async fn wait_for_both_started(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        let evs = events(daemon, run_id).await;
        if started_harness(&evs, "aaaaaaaa").is_some()
            && started_harness(&evs, "bbbbbbbb").is_some()
        {
            return evs;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("both entry nodes should have started within the timeout");
}

#[tokio::test]
async fn resolved_harness_is_frozen_on_node_started() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    let evs = wait_for_both_started(&daemon, &run_id).await;

    // The unpinned node froze the `claude` floor; the pinned node froze `opencode`
    // — the resolved harness, not what any tier says now (ADR-0046, gel au spawn).
    assert_eq!(started_harness(&evs, "aaaaaaaa").as_deref(), Some("claude"));
    assert_eq!(
        started_harness(&evs, "bbbbbbbb").as_deref(),
        Some("opencode")
    );
}

#[tokio::test]
async fn resume_reposes_the_frozen_harness() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_both_started(&daemon, &run_id).await;

    // Kill the pinned node's live session out-of-band (on the daemon's own tmux
    // socket) so the pane endpoint has to RESUME rather than merely capture.
    let socket = daemon.tmux_socket();
    let session = format!("pdo-{run_id}-bbbbbbbb-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();

    // The pane endpoint reads the frozen `opencode` harness back from `NodeStarted`,
    // finds it can resume, and re-poses it (via the command seam). It must not
    // answer a bare "session no longer available".
    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/bbbbbbbb/pane",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let pane: serde_json::Value = resp.json().await.unwrap();
    assert_ne!(
        pane["source"].as_str(),
        Some("unavailable"),
        "a resumable pinned node must re-pose, not report unavailable: {pane}"
    );
}
