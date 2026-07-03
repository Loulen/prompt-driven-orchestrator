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

// --- #251: stale sweep panic isolation + liveness health ---

async fn get_stale_health(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{}/stale/health", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// #251 root-cause regression: a panic inside one stale-detection sweep must NOT
/// silently disable detection for the daemon's life. The sweep runs under panic
/// isolation (`run_isolated`), so a panicking tick is contained — the driving
/// call returns NORMALLY — and the *next* sweep recovers and does real detection.
/// Pre-fix, the bare `loop { run_stale_detection().await }` let a single panic
/// kill the task, leaving every later stall (idle, dead-session, run-level)
/// undetected — exactly the silent idle-stall in the field.
#[tokio::test]
async fn a_panicking_stale_sweep_is_isolated_and_the_next_sweep_recovers() {
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

    // Kill the session out-of-band so a *working* sweep would mark the node Failed.
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !tmux_has_session(&socket, &session),
        "session should be dead after manual kill"
    );

    // Arm the one-shot poison AFTER boot so the immediate startup sweep doesn't
    // consume it. The next sweep will panic.
    daemon.arm_stale_panic();

    // Sweep 1 panics internally. The supervised seam contains it, so this call
    // returns NORMALLY — pre-fix the panic would have unwound this test task and
    // failed the test right here.
    daemon.run_stale_detection_tick().await;

    // The panic blinded detection on this tick: the dead-session node is still
    // Running (it was NOT marked Failed)...
    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("running"),
        "the panicking sweep must not have detected the dead session"
    );
    // ...yet the heartbeat advanced *through* the panic (written before it), so
    // `/stale/health` can prove the loop reached the sweep.
    let h1 = get_stale_health(&daemon).await;
    assert!(
        h1["last_tick_at"].as_str().is_some(),
        "the heartbeat must advance even through a panicking sweep: {h1}"
    );

    // Sweep 2 recovers (poison disarmed itself) and does real detection: the
    // dead-session node is now Failed with a cause naming the dead session.
    daemon.run_stale_detection_tick().await;
    let (status, reason) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("failed"),
        "the sweep must recover after a contained panic and detect the dead session"
    );
    let reason = reason.expect("failed node must carry a cause");
    assert!(
        reason.contains(&session),
        "failure cause {reason:?} must name the dead session {session:?}"
    );
}

/// #251 observability: `GET /stale/health` exposes the sweep's last tick and the
/// configured interval, and the timestamp advances as sweeps run — the missing
/// instrument (mirroring `/triggers/health`, #222) that distinguishes a dead
/// sweep from a per-node probe miss on the next idle-stall.
#[tokio::test]
async fn stale_health_reports_last_tick_and_advances() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let h = get_stale_health(&daemon).await;
    assert_eq!(
        h["tick_interval_secs"].as_u64(),
        Some(30),
        "health must report the configured sweep interval"
    );

    // Drive a sweep; last_tick_at becomes non-null.
    daemon.run_stale_detection_tick().await;
    let t1 = get_stale_health(&daemon).await["last_tick_at"]
        .as_str()
        .expect("last_tick_at set after a sweep")
        .to_string();

    // A later sweep advances it (canonical-UTC strings compare chronologically;
    // tolerate extra background-loop ticks — the value only moves forward).
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    daemon.run_stale_detection_tick().await;
    let t2 = get_stale_health(&daemon).await["last_tick_at"]
        .as_str()
        .expect("last_tick_at still set")
        .to_string();

    assert!(
        t2 > t1,
        "last_tick_at must advance across sweeps: {t1} then {t2}"
    );
}

// --- #290: blocked-on-usage-limit menu detection (observability only) ---

/// The Claude Code usage-limit interactive menu, painted verbatim into the node
/// pane by the tmux command override (literal `\n` so `printf` renders newlines).
const USAGE_LIMIT_MENU: &str =
    "What do you want to do?\\n  1. Stop and wait for limit to reset\\n  2. Switch to usage credits\\n";

/// Fetch the run's event log over HTTP and return the `node_blocked_on_limit`
/// events for `(node, iter)` — the durable, queryable surface the daemon uses to
/// record the observation (#290).
async fn blocked_on_limit_events(
    daemon_url: &str,
    run_id: &str,
    node: &str,
    iter: i64,
) -> Vec<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = resp.json().await.unwrap();
    events
        .into_iter()
        .filter(|e| {
            e["kind"].as_str() == Some("node_blocked_on_limit")
                && e["node_id"].as_str() == Some(node)
                && e["iter"].as_i64() == Some(iter)
        })
        .collect()
}

/// #290 core: a node whose pane is stuck on Claude Code's usage-limit menu is
/// SURFACED (gauge + one durable event) while KEPT `running` — never false-failed.
/// The override paints the menu into every node pane, so the node genuinely sits
/// on the blocking prompt (no real Claude session, no real 5-h wait). Recovery is
/// explicitly out of scope (Slice 2/3): the assertion is observability + no
/// false-positive, plus rising-edge de-dup across a second sweep.
#[tokio::test]
async fn usage_limit_menu_is_flagged_and_node_stays_running() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn_with_override(
        seed,
        Some(format!("printf '{USAGE_LIMIT_MENU}'; exec sleep 600")),
    )
    .await
    .unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "node session should appear after POST /runs"
    );
    // Give `printf` a moment to render the menu into the pane before we sweep.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Pre-condition: the node is Running (the menu is a host prompt, not a death).
    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(status.as_deref(), Some("running"));

    // One detector sweep observes the menu.
    daemon.run_stale_detection_tick().await;

    // Surfaced on the gauge …
    let health = get_stale_health(&daemon).await;
    assert_eq!(
        health["blocked_on_limit"].as_i64(),
        Some(1),
        "the stuck node must be counted on /stale/health"
    );

    // … and the node is STILL running (the core resilience assertion — a throttle
    // must never be mistaken for a break).
    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(
        status.as_deref(),
        Some("running"),
        "a menu-blocked node must NOT be failed/staled — observability only"
    );

    // … and exactly one durable event records the episode.
    let events = blocked_on_limit_events(&daemon.url(), &run_id, NODE_ID, 1).await;
    assert_eq!(
        events.len(),
        1,
        "exactly one node_blocked_on_limit event for (worker, iter 1)"
    );
    assert_eq!(
        events[0]["payload"]["signal"].as_str(),
        Some("usage_limit_menu"),
        "the event payload must carry the signal that flagged it"
    );

    // Rising-edge de-dup: a second sweep on the still-held menu must NOT emit a
    // second event, and the gauge stays at 1 (no per-tick climb / spam).
    daemon.run_stale_detection_tick().await;
    let health = get_stale_health(&daemon).await;
    assert_eq!(
        health["blocked_on_limit"].as_i64(),
        Some(1),
        "gauge must stay 1 across a second sweep (recomputed, not accumulating)"
    );
    let events = blocked_on_limit_events(&daemon.url(), &run_id, NODE_ID, 1).await;
    assert_eq!(
        events.len(),
        1,
        "rising-edge de-dup: still exactly one event after a second sweep"
    );

    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();
}

/// #290 negative control: a normal Running node (no menu in the pane) must NOT be
/// flagged — guards against a hair-trigger detector emitting false positives in
/// the wild. Default spawn paints nothing (`exec sleep 600`).
#[tokio::test]
async fn usage_limit_detector_does_not_flag_a_normal_running_node() {
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
    tokio::time::sleep(Duration::from_millis(300)).await;

    daemon.run_stale_detection_tick().await;

    let health = get_stale_health(&daemon).await;
    assert_eq!(
        health["blocked_on_limit"].as_i64(),
        Some(0),
        "an un-blocked node must not be counted (no false positive)"
    );
    assert!(
        blocked_on_limit_events(&daemon.url(), &run_id, NODE_ID, 1)
            .await
            .is_empty(),
        "an un-blocked node must emit no node_blocked_on_limit event"
    );
    let (status, _) = node_state(&daemon.url(), &run_id, NODE_ID).await;
    assert_eq!(status.as_deref(), Some("running"));

    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();
}
