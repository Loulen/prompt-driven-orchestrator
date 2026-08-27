//! #614 (correctif 2): resuming a node whose **frozen descriptor is gone**
//! REFUSES — it never relaunches `claude` in the node's worktree.
//!
//! Layer 3, through the daemon. A node is pinned to a disk-declared harness
//! (`ghost`), spawned through the tmux command seam (a harmless `sleep`, so the
//! ghost binary need never exist), and freezes `ghost` on `NodeStarted`. The disk
//! descriptor is then removed and the live session killed. `GET …/pane` must read
//! the frozen `ghost`, fail to resolve it, and refuse — naming the harness — rather
//! than silently re-enter this worktree on a `claude --continue`, which would run a
//! different agent than the node was started on.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "ghost-test";
const PIPELINE_YAML: &str = r#"name: ghost-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: cccccccc
    name: on-ghost
    type: doc-only
    pin_harness: ghost
    outputs:
      - name: out
    view: { x: 200, y: 120 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: cccccccc, port: task }
  - source: { node: cccccccc, port: out }
    target: { node: end, port: result }
"#;

/// A disk descriptor declaring `ghost` — leader-correct (`exec ghost …`), so it
/// loads, and resumable by identity (`--resume` fills `{resume}`).
const GHOST_DESCRIPTOR: &str = r#"harnesses:
  ghost:
    binary: ghost
    launch: ["exec", "ghost", "--auto", "--model {model}", "--session-id {session_id}", "{prompt}"]
    resume: ["exec", "ghost", "--auto", "{resume}"]
    resume_by_id: "--resume"
"#;

fn descriptors_path(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".pdo").join("harnesses").join("descriptors.yaml")
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("cccccccc.md"), "You are on ghost.\n")?;

    // Declare `ghost` on disk (home root == repo root under the home override).
    let dpath = descriptors_path(repo);
    std::fs::create_dir_all(dpath.parent().unwrap())?;
    std::fs::write(&dpath, GHOST_DESCRIPTOR)?;

    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
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
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn started_harness(daemon: &TestDaemon, run_id: &str, node: &str) -> Option<String> {
    let v: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v.as_array()?
        .iter()
        .rev()
        .find(|e| e["kind"] == "node_started" && e["node_id"] == node)
        .and_then(|e| e["payload"]["harness"].as_str())
        .map(String::from)
}

async fn wait_for_ghost_started(daemon: &TestDaemon, run_id: &str) {
    for _ in 0..50 {
        if started_harness(daemon, run_id, "cccccccc").await.as_deref() == Some("ghost") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the ghost-pinned node should have started within the timeout");
}

#[tokio::test]
async fn resume_of_a_gone_frozen_harness_refuses_never_relaunches_claude() {
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_ghost_started(&daemon, &run_id).await;

    // Kill the node's live session so `/pane` must RESUME rather than capture.
    let socket = daemon.tmux_socket();
    let session = format!("pdo-{run_id}-cccccccc-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();

    // Remove the disk descriptor: `ghost` no longer resolves in any tier.
    std::fs::remove_file(descriptors_path(daemon.repo_root())).unwrap();

    // The pane endpoint reads the frozen `ghost`, cannot resolve it, and REFUSES —
    // naming the harness — instead of relaunching claude in this worktree.
    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/cccccccc/pane",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let pane: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        pane["source"].as_str(),
        Some("unavailable"),
        "a gone frozen harness must refuse, not resume: {pane}"
    );
    assert_eq!(pane["resumed"].as_bool(), Some(false), "{pane}");
    let content = pane["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("ghost") && content.contains("no longer resolves"),
        "the refusal must name the missing harness: {content}"
    );
}
