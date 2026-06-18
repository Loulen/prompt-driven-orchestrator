//! Layer 3a — process lifecycle tests for issue #213.
//!
//! Covers the four chantiers of "Pérennisation C":
//! 1. Liveness sweep: a Running node whose tmux session dies is marked Failed
//!    with a cause naming the dead session, within one detector cycle.
//! 2. Reap on terminal state: a completed/failed/stopped node's session is
//!    killed and a pane snapshot is kept so /pane keeps serving it.
//! 3. Boot recovery: a Running node orphaned across a daemon restart is marked
//!    Failed with a cause at boot.
//! 4. Atomic admission: concurrent spawns never exceed the session cap.

mod common;

use std::time::Duration;

use common::TestDaemon;
use pdo_daemon::tmux_session_manager;

const PIPELINE_NAME: &str = "lifecycle-c";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: lifecycle-c
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
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
    target: { node: worker, port: in }
"#;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmux_has_session(socket: &str, session: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", socket, "has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

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

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "test input" });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn wait_for_session(socket: &str, session: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tmux_has_session(socket, session) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn wait_for_session_gone(socket: &str, session: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !tmux_has_session(socket, session) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Write the worker's required `out` artifact so output validation passes, then
/// POST node_done.
async fn complete_worker(daemon: &TestDaemon, run_id: &str) {
    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts/worker/iter-1/out");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Output\nDone.").unwrap();

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/done",
            daemon.url()
        ))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "node_done should succeed");
}

/// Fetch the projected status + failure_reason of `node` in `run_id`.
async fn node_state(
    daemon_url: &str,
    run_id: &str,
    node: &str,
) -> (Option<String>, Option<String>) {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    let status = json["nodes"][node]["status"].as_str().map(String::from);
    let reason = json["nodes"][node]["failure_reason"]
        .as_str()
        .map(String::from);
    (status, reason)
}

/// AC1: killing a Running node's tmux session out-of-band causes the next
/// detector sweep to mark the node Failed, with a cause naming the dead
/// session. The transition travels through the #212 guard (the failure event
/// goes via `append_event`).
#[tokio::test]
async fn dead_session_marks_node_failed_with_session_cause() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    // Pre-condition: the node is Running with a live session.
    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(status.as_deref(), Some("running"));

    // Kill the session out-of-band (tmux server crash / OOM / manual kill).
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !tmux_has_session(&socket, &session),
        "session should be dead after manual kill"
    );

    // One detector cycle.
    daemon.run_stale_detection_tick().await;

    let (status, reason) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("failed"),
        "node with a dead session must be marked Failed within one detector cycle"
    );
    let reason = reason.expect("failed node must carry a cause");
    assert!(
        reason.contains(&session),
        "failure cause {reason:?} must name the dead session {session:?}"
    );
}

/// AC1 invariant: a nominal Running node whose session is alive is NEVER
/// touched by the detector — no false-positive Failed.
#[tokio::test]
async fn live_session_node_is_not_failed_by_detector() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    // Several detector cycles while the session stays alive (sleep override).
    for _ in 0..3 {
        daemon.run_stale_detection_tick().await;
    }

    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("running"),
        "a node with a live session must never be failed by the detector"
    );

    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();
}

/// Fetch the projected run-level status of `run_id`.
async fn run_status(daemon_url: &str, run_id: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["status"].as_str().map(String::from)
}

/// #214 invariant (run-level stall): once a node is Failed and the run has no
/// live node and nothing the scheduler can spawn, the run must NOT sit Running
/// forever. The periodic stale-detection sweep reconciles it to a terminal
/// state (Failed) with a run-level cause — never a silent stall.
#[tokio::test]
async fn run_with_no_live_node_and_nothing_schedulable_is_reconciled_terminal() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    // The run is Running with one live node.
    assert_eq!(
        run_status(&daemon.url(), &run_id).await.as_deref(),
        Some("running")
    );

    // Kill the only node's session: the next sweep fails the node. With
    // `worker` Failed, `end` can never receive its input — no live node, nothing
    // schedulable. The run is wedged.
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // One sweep fails the node AND reconciles the now-wedged run.
    daemon.run_stale_detection_tick().await;

    let (node_status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        node_status.as_deref(),
        Some("failed"),
        "the node with the dead session must be Failed"
    );

    let status = run_status(&daemon.url(), &run_id).await;
    assert_eq!(
        status.as_deref(),
        Some("failed"),
        "a run with no live node and nothing schedulable must be reconciled \
         terminal, not left Running forever (silent stall)"
    );
}

/// #214 invariant (boot path): a run left Running with a Failed node and nothing
/// schedulable across a daemon restart is reconciled terminal at boot, instead
/// of staying Running forever. Mirrors the two fixture runs (295be69, ec7c3ff)
/// that were stuck Running after a mid-run kill.
#[tokio::test]
async fn boot_recovery_reconciles_a_run_level_stall() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    // Simulate the crash: the session vanishes while the node is still Running.
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Boot reconciliation: orphaned node Failed, THEN the run-level stall it
    // leaves behind reconciled terminal in the same boot pass.
    daemon.run_boot_recovery_tick().await;

    let (node_status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        node_status.as_deref(),
        Some("failed"),
        "the orphaned node must be Failed at boot"
    );

    let status = run_status(&daemon.url(), &run_id).await;
    assert_eq!(
        status.as_deref(),
        Some("failed"),
        "a run wedged behind a boot-failed node must be reconciled terminal at \
         boot, not left Running forever"
    );
}

/// AC3: a Running node orphaned across a daemon restart — its tmux session no
/// longer exists at boot — is reconciled to Failed with a cause naming the
/// session, fail-fast, instead of staying Running forever and burning a slot.
#[tokio::test]
async fn boot_recovery_fails_orphaned_running_node() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    // Simulate the crash: the session vanishes (tmux server died) while the
    // event log still says the node is Running.
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!tmux_has_session(&socket, &session));

    // Boot-time reconciliation (the same pass the daemon runs at startup).
    daemon.run_boot_recovery_tick().await;

    let (status, reason) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("failed"),
        "an orphaned Running node must be reconciled to Failed at boot"
    );
    let reason = reason.expect("recovered node must carry a cause");
    assert!(
        reason.contains(&session),
        "recovery cause {reason:?} must name the orphaned session {session:?}"
    );
}

/// AC2 / #205: when a node reaches a terminal state (completed here), its tmux
/// session is reaped promptly (not after the 1h TTL) and a pane snapshot is
/// kept. The /pane endpoint then serves the snapshot, flagged so the caller
/// knows it is a post-mortem and not a live attach.
#[tokio::test]
async fn completed_node_session_is_reaped_and_pane_serves_snapshot() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );

    complete_worker(&daemon, &run_id).await;

    // Reaped promptly on the terminal transition — no waiting for the TTL.
    assert!(
        wait_for_session_gone(&socket, &session, Duration::from_secs(5)).await,
        "a completed node's session must be reaped on the terminal transition"
    );

    // The snapshot file exists in the node dir (survives worktree removal).
    let snapshot = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes/worker/pane-iter-1.snapshot");
    assert!(
        snapshot.exists(),
        "a pane snapshot must be persisted at {snapshot:?} when the session is reaped"
    );

    // /pane serves the snapshot, flagged as such, and does NOT resurrect a live
    // session.
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/pane?iter=1",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        json["source"].as_str(),
        Some("snapshot"),
        "the pane endpoint must report it served the persisted snapshot"
    );
    assert!(
        !tmux_has_session(&socket, &session),
        "serving a snapshot must not resurrect the reaped session"
    );
}
