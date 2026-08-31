//! Layer 3a — retrying a node **inside a bounded loop** is loop-aware.
//!
//! The generic retry recipe (stop → invalidate downstream → re-spawn at `iter+1`)
//! is loop-blind, and both halves misfire inside a cycle:
//!
//! 1. **The lap counter is preserved.** A member's `iter` IS its lap index, so
//!    re-running it must land on the SAME lap, never `iter+1` — bumping it forges a
//!    phantom lap that drags the region toward its `max_iter`.
//! 2. **The same-lap upstream is spared.** The downstream walk must stop at the
//!    region's re-entry (back) edge, so retrying a later member never resets an
//!    earlier member of the *running* lap whose output is already validated.
//!
//! Both are driven against a real daemon over a review-loop pipeline (`impl -> rev`
//! with a `rev -> impl` else back-edge, auto-covered by a bounded region).

use std::process::Command;
use std::time::Duration;

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "retry-loop-test";

/// `impl -> rev`, with `rev -> impl` as the else (continuation) back-edge and
/// `rev -> end` as the `iter >= max` exit. `detect_cycles` closes {impl, rev} into
/// the declared bounded region. `impl` is the entry (fed from outside by `start`).
const PIPELINE_YAML: &str = r#"name: retry-loop-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: impl
    name: impl
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: code
  - id: rev
    name: rev
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: impl, port: task }
  - source: { node: impl, port: code }
    target: { node: rev, port: code }
  - source: { node: rev, port: review }
    target: { node: impl, port: task }
    else: true
  - source: { node: rev, port: review }
    target: { node: end, port: result }
    when: "iter >= max"
loops:
  - id: review_loop
    kind: bounded
    members: [rev, impl]
    max_iter: 3
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
    for id in ["impl", "rev"] {
        std::fs::write(
            prompts_dir.join(format!("{id}.md")),
            "You are a test node.\n",
        )?;
    }
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
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert_eq!(status, 201, "POST /runs should succeed, got: {text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn events_of(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let resp = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array().cloned().unwrap_or_default()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn node_status(daemon: &TestDaemon, run_id: &str, node_id: &str) -> String {
    get_run(daemon, run_id).await["nodes"][node_id]["status"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Every `(node_id, iter)` the run has ever started.
async fn started_pairs(daemon: &TestDaemon, run_id: &str) -> Vec<(String, i64)> {
    events_of(daemon, run_id)
        .await
        .into_iter()
        .filter(|e| e["kind"] == "node_started")
        .map(|e| {
            (
                e["node_id"].as_str().unwrap_or_default().to_string(),
                e["iter"].as_i64().unwrap_or_default(),
            )
        })
        .collect()
}

async fn wait_until<F, Fut>(what: &str, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if pred().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn wait_for_node_status(daemon: &TestDaemon, run_id: &str, node_id: &str, want: &str) {
    wait_until(&format!("{node_id} to become {want}"), || async {
        node_status(daemon, run_id, node_id).await == want
    })
    .await;
}

/// Completes `impl` at lap 1 by depositing its `code` output and posting `/done`,
/// so the scheduler spawns `rev` at lap 1. Returns the artifact dir so the caller
/// can assert it survives a later retry.
async fn complete_impl_lap1(daemon: &TestDaemon, run_id: &str) -> std::path::PathBuf {
    wait_for_node_status(daemon, run_id, "impl", "running").await;
    let code_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts/impl/iter-1/code");
    std::fs::create_dir_all(&code_dir).unwrap();
    std::fs::write(code_dir.join("output.md"), "# code\n").unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/impl/done", daemon.url()))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "POST impl/done");
    wait_for_node_status(daemon, run_id, "impl", "completed").await;
    wait_for_node_status(daemon, run_id, "rev", "running").await;
    code_dir
}

fn kill_session(daemon: &TestDaemon, run_id: &str, node_id: &str, iter: i64) {
    let _ = Command::new("tmux")
        .args([
            "-L",
            &daemon.tmux_socket(),
            "kill-session",
            "-t",
            &format!("pdo-{run_id}-{node_id}-iter-{iter}"),
        ])
        .output();
}

/// Defect ② — the preview promises exactly what the retry will reset. Retrying
/// `rev` (a later loop member) must NOT list `impl`, the same-lap upstream reached
/// only through the back-edge; it lists only the genuine forward slice (`end`).
#[tokio::test]
async fn retry_preview_of_a_loop_member_spares_the_same_lap_upstream() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    complete_impl_lap1(&daemon, &run_id).await;

    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/rev/retry/preview",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let downstream: Vec<String> = body["downstream"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    assert!(
        !downstream.contains(&"impl".to_string()),
        "the same-lap upstream `impl` must NOT be in the retry preview: {downstream:?}"
    );
    assert!(
        downstream.contains(&"end".to_string()),
        "the genuine forward slice `end` must be in the preview: {downstream:?}"
    );

    kill_session(&daemon, &run_id, "rev", 1);
}

/// Defect ① + ② together — retrying `rev` re-runs the SAME lap (`iter 1`, not
/// `iter 2`) and leaves `impl` (its already-validated same-lap producer) fully
/// intact: still completed, artifact on disk, no `NodeInvalidated`.
#[tokio::test]
async fn retry_of_a_loop_member_reuses_the_lap_and_spares_the_upstream() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    let impl_artifact = complete_impl_lap1(&daemon, &run_id).await;

    let before = events_of(&daemon, &run_id).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/rev/retry", daemon.url()))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true, "{body}");

    // ① Same lap — the re-spawn lands on iter 1, never iter 2.
    assert_eq!(
        body["iter"], 1,
        "a loop member re-runs the SAME lap, not iter+1: {body}"
    );
    assert_eq!(body["spawned"][0]["node_id"], "rev", "{body}");
    assert_eq!(body["spawned"][0]["iter"], 1, "{body}");

    // ② The invalidation set never reaches back to the same-lap upstream.
    let invalidated: Vec<String> = body["invalidated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        !invalidated.contains(&"impl".to_string()),
        "retry must not invalidate the same-lap upstream `impl`: {invalidated:?}"
    );

    // The event log agrees: no `NodeInvalidated` for `impl`, and `rev` re-spawned
    // at iter 1 (never iter 2).
    wait_until("rev to re-spawn at lap 1", || async {
        started_pairs(&daemon, &run_id)
            .await
            .iter()
            .filter(|(id, it)| id == "rev" && *it == 1)
            .count()
            >= 2
    })
    .await;
    let after = events_of(&daemon, &run_id).await;
    let new_events = &after[before.len().min(after.len())..];
    assert!(
        !new_events
            .iter()
            .any(|e| e["kind"] == "node_invalidated" && e["node_id"] == "impl"),
        "no NodeInvalidated for `impl`: {new_events:#?}"
    );
    assert!(
        !started_pairs(&daemon, &run_id)
            .await
            .contains(&("rev".to_string(), 2)),
        "rev must never have started a phantom lap 2"
    );

    // `impl` is untouched: still completed, artifact still on disk.
    assert_eq!(node_status(&daemon, &run_id, "impl").await, "completed");
    assert!(
        impl_artifact.join("output.md").exists(),
        "impl's lap-1 artifact must survive the retry of rev"
    );

    kill_session(&daemon, &run_id, "rev", 1);
}
