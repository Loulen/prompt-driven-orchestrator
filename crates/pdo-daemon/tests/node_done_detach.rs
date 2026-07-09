//! Layer 3a — proves #304 (ADR-0023) is fixed: the post-terminal-event tail of
//! `node_done` / `node_fail` / `node_skip` (session reap + run advance /
//! `RunFailed` / `RunSkipped`) is DETACHED from the HTTP request future, so a
//! client disconnect mid-window — including the self-inflicted one where the
//! reap kills the `pdo complete` client's own tmux session — can no longer
//! cancel the advance.
//!
//! Determinism: hyper only cancels the in-flight future at its next `.await`
//! after the FIN, so a bare "drop the socket fast" race is probabilistic. The
//! `arm_node_done_gate` seam (sibling of `arm_spawn_panic`, #279) parks the
//! tail at its head — the FIRST instruction of the detached task — while the
//! test drops a raw TCP connection, then releases it. Under the pre-#304 inline
//! code (or a reorder-only fix) the gate wait sits in the request future: the
//! drop cancels it and releasing changes nothing → these tests go red. Under
//! DETACH the tail survives the drop and resumes on release → green. That
//! placement is the whole discriminator; do not "simplify" it away.
//!
//! Must run through `TestDaemon` (a real TCP listener): the in-lib `oneshot`
//! router tests poll the handler future to completion regardless of any client,
//! so the cancellation bug is invisible there.

mod common;

use std::time::Duration;

use common::TestDaemon;
use tokio::io::AsyncWriteExt;

const PIPELINE_NAME: &str = "detach-tail";
// start → a → b → end; doc-only nodes so completion needs no sub-worktree merge.
const PIPELINE_YAML: &str = r#"name: detach-tail
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: a
    name: a
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
  - id: b
    name: b
    type: doc-only
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
    target: { node: a, port: in }
  - source: { node: a, port: out }
    target: { node: b, port: in }
  - source: { node: b, port: out }
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

async fn create_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "test input" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
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

/// Poll the run's events until `pred` matches one, or time out.
async fn wait_for_event<F>(daemon: &TestDaemon, run_id: &str, what: &str, pred: F)
where
    F: Fn(&serde_json::Value) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if events(daemon, run_id).await.iter().any(&pred) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what} in run {run_id}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Satisfy output validation for a doc-only node before `pdo complete`: write
/// the artifact its declared `out` port expects, exactly where the agent would.
fn write_out_artifact(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    let dir = daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
        .join(".pdo")
        .join("artifacts")
        .join(node_id)
        .join("iter-1")
        .join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "artifact\n").unwrap();
}

/// Send a raw HTTP POST over a bare `TcpStream` and return the still-open
/// stream WITHOUT reading the response. reqwest can't do this: the test must
/// control exactly when the connection drops (mid-window, before the response).
async fn raw_post(daemon: &TestDaemon, path: &str, body: &str) -> tokio::net::TcpStream {
    let mut stream = tokio::net::TcpStream::connect(daemon.addr).await.unwrap();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    stream
}

/// Drive one terminal transition through the drop-mid-window sequence:
/// arm the gate → raw POST → wait for the in-request terminal event (proof the
/// append happened and the tail is parked at the gate) → drop the socket (FIN
/// before any response byte) → give hyper a beat to notice → release the gate.
async fn terminal_transition_over_dropped_connection(
    daemon: &TestDaemon,
    run_id: &str,
    path: &str,
    body: &str,
    terminal_event: &str,
    node_id: &str,
) {
    daemon.arm_node_done_gate();
    let stream = raw_post(daemon, path, body).await;
    wait_for_event(daemon, run_id, terminal_event, |e| {
        e["kind"] == terminal_event && e["node_id"] == node_id
    })
    .await;
    drop(stream);
    // Let the FIN propagate so hyper cancels the request future (pre-fix) while
    // the tail is still parked — this is the window the bug lives in.
    tokio::time::sleep(Duration::from_millis(300)).await;
    daemon.release_node_done_gate();
}

#[tokio::test]
async fn node_done_survives_client_disconnect_and_spawns_successor() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    // Entry node `a` is spawned by run creation.
    wait_for_event(&daemon, &run_id, "node_started for a", |e| {
        e["kind"] == "node_started" && e["node_id"] == "a"
    })
    .await;

    // Flush the run-creation `pipeline_modified` debounce (~1 s): its
    // `spawn_ready_after_event` re-drive must land while `b` is NOT ready yet
    // (a no-op), or it would rescue the cancelled advance and mask the bug.
    tokio::time::sleep(Duration::from_secs(2)).await;

    write_out_artifact(&daemon, &run_id, "a");
    terminal_transition_over_dropped_connection(
        &daemon,
        &run_id,
        &format!("/runs/{run_id}/nodes/a/done"),
        r#"{"iter": 1}"#,
        "node_completed",
        "a",
    )
    .await;

    // The advance must survive the dropped connection: successor `b` spawns.
    wait_for_event(&daemon, &run_id, "node_started for successor b", |e| {
        e["kind"] == "node_started" && e["node_id"] == "b"
    })
    .await;
}

#[tokio::test]
async fn end_node_completes_run_over_dropped_connection() {
    // The end-port wrinkle (recurrence e3e5ac3): the LAST node's completion
    // fires the end-port deposit + `RunCompleted` inside the same cancelled
    // future — the variant that escapes even the #279 reconciler.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    wait_for_event(&daemon, &run_id, "node_started for a", |e| {
        e["kind"] == "node_started" && e["node_id"] == "a"
    })
    .await;

    // Complete `a` normally (gate unarmed) so `b` spawns.
    write_out_artifact(&daemon, &run_id, "a");
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/a/done", daemon.url()))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    wait_for_event(&daemon, &run_id, "node_started for b", |e| {
        e["kind"] == "node_started" && e["node_id"] == "b"
    })
    .await;

    write_out_artifact(&daemon, &run_id, "b");
    terminal_transition_over_dropped_connection(
        &daemon,
        &run_id,
        &format!("/runs/{run_id}/nodes/b/done"),
        r#"{"iter": 1}"#,
        "node_completed",
        "b",
    )
    .await;

    wait_for_event(&daemon, &run_id, "run_completed", |e| {
        e["kind"] == "run_completed"
    })
    .await;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if run_status(&daemon, &run_id).await.as_deref() == Some("completed") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "run must reach status `completed`, not wedge `running` with all nodes done"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn node_fail_emits_run_failed_over_dropped_connection() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    wait_for_event(&daemon, &run_id, "node_started for a", |e| {
        e["kind"] == "node_started" && e["node_id"] == "a"
    })
    .await;

    terminal_transition_over_dropped_connection(
        &daemon,
        &run_id,
        &format!("/runs/{run_id}/nodes/a/fail"),
        r#"{"iter": 1, "reason": "boom"}"#,
        "node_failed",
        "a",
    )
    .await;

    // Pre-#304 the `RunFailed` append lived past the reap in the cancelled
    // future: the node was Failed but the run stayed `running` forever.
    wait_for_event(&daemon, &run_id, "run_failed", |e| {
        e["kind"] == "run_failed"
    })
    .await;
}

#[tokio::test]
async fn node_skip_emits_run_skipped_over_dropped_connection() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    wait_for_event(&daemon, &run_id, "node_started for a", |e| {
        e["kind"] == "node_started" && e["node_id"] == "a"
    })
    .await;

    terminal_transition_over_dropped_connection(
        &daemon,
        &run_id,
        &format!("/runs/{run_id}/nodes/a/skip"),
        r#"{"iter": 1, "reason": "nothing to do"}"#,
        "node_completed",
        "a",
    )
    .await;

    wait_for_event(&daemon, &run_id, "run_skipped", |e| {
        e["kind"] == "run_skipped"
    })
    .await;
}
