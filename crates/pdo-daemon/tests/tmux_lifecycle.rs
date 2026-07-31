//! Layer 3a — tmux lifecycle tests for issue #23.
//!
//! Tests:
//! 1. Reaper kills sessions for NodeRuns completed > TTL ago.
//! 2. Orphan sweep at boot kills pre-existing stale pdo-* sessions.
//! 3. Dead-session re-spawn: kill a session, hit /pane, assert fresh session.

mod common;

use std::sync::Mutex;
use std::time::Duration;

use common::TestDaemon;
use pdo_daemon::tmux_session_manager;

/// Tests in this file mutate process-wide env vars
/// (PDO_REAPER_*_SECS, PDO_DAEMON_NO_CLEANUP) and assert on
/// timing-sensitive reaper behaviour. They MUST run serially or one test will
/// see another's values. (The tmux command override is per-daemon config now —
/// `TestDaemon::spawn`'s harmless `sleep` default — not a process-global env.)
static SERIAL: Mutex<()> = Mutex::new(());

fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

const PIPELINE_NAME: &str = "lifecycle-test";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: lifecycle-test
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

fn create_fake_tmux_session(socket: &str, name: &str) {
    let _ = std::process::Command::new("tmux")
        .args([
            "-L",
            socket,
            "new-session",
            "-d",
            "-s",
            name,
            "sleep",
            "300",
        ])
        .output();
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

async fn create_run(daemon: &TestDaemon) -> String {
    // #470: the target repo is required at the create boundary (ADR-0033).
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
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Layer 3a: A completed node's session is reaped on the terminal transition
/// (#205/#213) — NOT left alive until the 1h TTL as the superseded #23
/// behaviour did. A pane snapshot is kept for post-mortem inspection. Uses a
/// long TTL so this asserts the terminal-state reap, not the periodic reaper.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn completed_node_session_is_reaped_on_terminal_transition() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    // Long TTL + interval so the periodic reaper can't be the one doing the
    // kill: only the terminal-state reap (#205) can.
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "session should appear after POST /runs"
    );

    // Create the required output file so output validation passes (refs #36).
    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/worker/iter-1/out");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Output\nDone.").unwrap();

    // Complete the node — the session is reaped on the terminal transition.
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/done",
            daemon.url()
        ))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // The session is gone promptly — the terminal reap, not the 1h TTL.
    assert!(
        wait_for_session_gone(&socket, &session, Duration::from_secs(5)).await,
        "session should be reaped on the terminal transition (#205), not held for the TTL"
    );

    // A pane snapshot survives for post-mortem inspection.
    let snapshot = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes/worker/pane-iter-1.snapshot");
    assert!(
        snapshot.exists(),
        "a pane snapshot must be persisted when the session is reaped"
    );

    // Clean up env
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a: At daemon boot, pre-existing orphan pdo-* sessions get swept.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn orphan_sweep_at_boot_kills_stale_session() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "0");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "1");

    // Boot the daemon first so we know which tmux socket to seed the
    // orphan on. Per-daemon socket isolation (post-#86) means the sweep
    // can only see sessions on its own socket — `default` would be a
    // different tmux server entirely.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();

    // Seed an orphan on the daemon's socket. This run_id isn't in the
    // event log, so the next reaper tick should kill the session.
    let orphan_session = "pdo-20260101-120000-aaaaaaa-orphan-iter-1";
    create_fake_tmux_session(&socket, orphan_session);
    assert!(
        tmux_has_session(&socket, orphan_session),
        "pre-condition: fake session should exist on daemon's socket"
    );

    // Wait for the reaper to sweep it (interval=1s).
    assert!(
        wait_for_session_gone(&socket, orphan_session, Duration::from_secs(5)).await,
        "orphan session should be killed by the periodic reaper (run absent from event log)"
    );

    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a: A daemon spawned with `PDO_DAEMON_NO_CLEANUP=1` (mirrors
/// what happens when a sub-claude accidentally runs `pdo daemon` —
/// `PDO_NODE_ID` is set in its env by `wrap_with_env`) MUST NOT reap
/// any orphan session, even one its own socket can see. Pinned by #86
/// follow-up: the only safe behaviour for a nested daemon is to be
/// completely passive on tmux state.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn nested_daemon_skips_orphan_sweep_and_reaper() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "0");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "1");
    std::env::set_var("PDO_DAEMON_NO_CLEANUP", "1");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();

    let orphan_session = "pdo-20260101-120000-aaaaaaa-orphan-iter-1";
    create_fake_tmux_session(&socket, orphan_session);
    assert!(
        tmux_has_session(&socket, orphan_session),
        "pre-condition: fake session should exist on daemon's socket"
    );

    // Wait 3× the reaper interval. If the reaper were running it would
    // have fired ~3 times by now; with no-cleanup mode it must not fire.
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(
        tmux_has_session(&socket, orphan_session),
        "nested daemon must NOT sweep orphans (PDO_DAEMON_NO_CLEANUP=1)"
    );

    // Cleanup
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", orphan_session])
        .output();
    std::env::remove_var("PDO_DAEMON_NO_CLEANUP");
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a: Kill a session manually, hit /pane, assert a fresh session appears.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn dead_session_respawn_via_pane_endpoint() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    // Long TTL so the reaper doesn't interfere
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "session should appear after POST /runs"
    );

    // Kill the session manually
    tmux_session_manager::kill(&socket, &session);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !tmux_has_session(&socket, &session),
        "session should be dead after manual kill"
    );

    // Hit the /pane endpoint — should re-spawn via resume
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
    assert!(json["content"].is_string());
    assert!(!json["content"].as_str().unwrap().is_empty());

    // The session should now exist again
    assert!(
        tmux_has_session(&socket, &session),
        "session should be re-spawned after /pane request"
    );

    // Clean up
    tmux_session_manager::kill(&socket, &session);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

// ---------------------------------------------------------------------------
// #485 / ADR-0038 — the sweep must not kill the session it just spawned
//
// Driven through the deterministic `run_orphan_sweep_tick()` seam, never by
// racing the 60 s interval. Honest limit, stated in the PR: no layer-3 test can
// *reproduce* the race — there is no hook between the inventory and the log read.
// The order is guaranteed by construction (the inventory is a parameter of
// `decide_sweep`) plus the invariant comments; these tests pin the observable
// consequences on both sides — the live session survives, the real orphan dies.
// ---------------------------------------------------------------------------

const TWO_NODE_PIPELINE: &str = "two-step";
const TWO_NODE_YAML: &str = r#"name: two-step
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
  - id: second
    name: second
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
  - source: { node: worker, port: out }
    target: { node: second, port: in }
  - source: { node: second, port: out }
    target: { node: end, port: result }
"#;

fn seed_two_node(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{TWO_NODE_PIPELINE}.yaml")),
        TWO_NODE_YAML,
    )?;
    git_init_with_commit(repo)?;
    Ok(())
}

async fn create_run_of(daemon: &TestDaemon, pipeline: &str) -> String {
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn run_events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/events", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array().cloned().unwrap_or_default()
}

/// Has `NodeStarted` for (node, iter) already landed in the persisted log?
async fn node_started_is_recorded(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> bool {
    run_events(daemon, run_id).await.iter().any(|e| {
        e["kind"] == "node_started" && e["node_id"] == node_id && e["iter"].as_i64() == Some(iter)
    })
}

async fn reaper_gauge(daemon: &TestDaemon) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/sessions", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["reaper"].clone()
}

/// Layer 3a (#485): the session of a node that IS in the event log survives an
/// **immediate** sweep tick. This is the shape of the bug — before the fix the
/// sweep resolved live sessions against a snapshot taken before they were born
/// and killed them within ~150 ms of their own spawn.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn live_node_session_survives_an_immediate_sweep_tick() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    // Long TTL + interval: the only sweep that runs is the one we drive, and the
    // TTL arm cannot be the thing under test.
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "session should appear after POST /runs"
    );

    // Three back-to-back ticks: any of them reading the log before the inventory
    // would classify this live session as an orphan.
    for _ in 0..3 {
        daemon.run_orphan_sweep_tick().await;
    }

    assert!(
        tmux_has_session(&socket, &session),
        "#485 REGRESSION: the sweep killed a live node's session"
    );
    let gauge = reaper_gauge(&daemon).await;
    assert_eq!(
        gauge["killed_for_absent_run"].as_i64(),
        Some(0),
        "no absence verdict may be reached in steady state, got {gauge}"
    );

    tmux_session_manager::kill(&socket, &session);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a (#485) — **the negative control, and it matters more than the test
/// above.** A fix that simply neutralised the sweep would pass that one. All
/// three absence arms plus the unparseable-name arm still kill, and the
/// `GET /sessions` reaper gauge counts them.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn genuine_orphans_are_still_reaped_and_counted() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();

    // The boot sweep has already run (it precedes `build_router`), so the gauge is
    // populated and quiet: nothing was there to kill.
    let before = reaper_gauge(&daemon).await;
    assert!(
        before["last_sweep_at"].is_string(),
        "the boot sweep must have published a timestamp, got {before}"
    );
    assert_eq!(
        before["killed"].as_i64(),
        Some(0),
        "a fresh daemon's boot sweep has nothing to kill, got {before}"
    );

    // A syntactically valid run_id that exists in no event log, on all four arms.
    let ghost = "20200101-000000-deadbee";
    let orphans = [
        tmux_session_manager::node_session_name(ghost, "zzTESTzz", 1),
        tmux_session_manager::manager_session_name(ghost),
        tmux_session_manager::shell_session_name(ghost),
        "pdo-ceci-nest-pas-un-nom".to_string(),
    ];
    for name in &orphans {
        create_fake_tmux_session(&socket, name);
        assert!(
            tmux_has_session(&socket, name),
            "pre-condition: {name} should exist on the daemon's socket"
        );
    }

    daemon.run_orphan_sweep_tick().await;

    for name in &orphans {
        assert!(
            !tmux_has_session(&socket, name),
            "REGRESSION: the sweep spared a genuine orphan ({name}) — sessions would \
             pile up toward the tmux collapse point"
        );
    }

    let after = reaper_gauge(&daemon).await;
    assert!(
        after["last_sweep_at"].as_str() > before["last_sweep_at"].as_str(),
        "the sweep must advance its timestamp: {before} → {after}"
    );
    assert_eq!(
        after["killed"].as_i64(),
        Some(4),
        "all four orphans should be tallied, got {after}"
    );
    assert_eq!(
        after["killed_for_absent_run"].as_i64(),
        Some(4),
        "all four are absence verdicts, got {after}"
    );

    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a (#485, slice A — `force_spawn_node`, the UI **Start** button): the
/// reservation is in the persisted log *before* the session exists, so an
/// immediate sweep tick cannot see a session without one. Before the fix the
/// primitive spawned first and returned the event for the caller to append, and
/// `force_spawn_node` targets a node with no history at all — so the reaper's
/// lookup found no entry for it.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn force_spawn_reserves_before_it_spawns() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed_two_node).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run_of(&daemon, TWO_NODE_PIPELINE).await;

    // `second` is downstream of a still-running `worker`, so it has no entry in
    // the projection at all — exactly the force-spawn exposure.
    assert!(
        wait_for_session(
            &socket,
            &tmux_session_manager::node_session_name(&run_id, "worker", 1),
            Duration::from_secs(5)
        )
        .await,
        "pre-condition: the entry node should be running"
    );
    assert!(
        !node_started_is_recorded(&daemon, &run_id, "second", 1).await,
        "pre-condition: `second` must be un-started"
    );

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/second/start", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Start should be accepted");

    let session = tmux_session_manager::node_session_name(&run_id, "second", 1);
    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "Start should spawn the session"
    );
    assert!(
        node_started_is_recorded(&daemon, &run_id, "second", 1).await,
        "#485 slice A: the reservation must already be readable once the session exists"
    );

    for _ in 0..3 {
        daemon.run_orphan_sweep_tick().await;
    }
    assert!(
        tmux_has_session(&socket, &session),
        "#485 REGRESSION: the sweep killed a force-spawned session"
    );

    tmux_session_manager::kill(&socket, &session);
    tmux_session_manager::kill(
        &socket,
        &tmux_session_manager::node_session_name(&run_id, "worker", 1),
    );
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a (#485, slice A — `node_retry`, the UI **Retry** button): same
/// invariant on the more exposed of the two paths. Retry appends
/// `NodeInvalidated` for the node itself, and that applier does an unconditional
/// `nodes.remove(node_id)` — so during the old spawn-then-append window the
/// reaper's lookup found no entry at *every* iteration, not just the first.
#[tokio::test]
// Holds the process-wide `serial_guard()` MutexGuard across `.await`s to keep
// the env-var-sensitive reaper tests from racing each other — intentional, and
// the same allow the rest of the crate uses for serialized async tests.
#[allow(clippy::await_holding_lock)]
async fn retry_reserves_before_it_spawns() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let _serial = serial_guard();

    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let run_id = create_run(&daemon).await;
    let iter1 = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(
        wait_for_session(&socket, &iter1, Duration::from_secs(5)).await,
        "pre-condition: iter 1 should be running"
    );

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/retry",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Retry should be accepted");

    let iter2 = tmux_session_manager::node_session_name(&run_id, NODE_ID, 2);
    assert!(
        wait_for_session(&socket, &iter2, Duration::from_secs(5)).await,
        "Retry should spawn iter 2"
    );
    assert!(
        node_started_is_recorded(&daemon, &run_id, NODE_ID, 2).await,
        "#485 slice A: the retry's reservation must already be readable once its \
         session exists"
    );

    for _ in 0..3 {
        daemon.run_orphan_sweep_tick().await;
    }
    assert!(
        tmux_has_session(&socket, &iter2),
        "#485 REGRESSION: the sweep killed a retried session"
    );

    tmux_session_manager::kill(&socket, &iter2);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}
