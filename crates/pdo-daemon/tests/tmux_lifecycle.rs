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

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
    });
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
    let run_id = create_run(&daemon.url()).await;
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
    let run_id = create_run(&daemon.url()).await;
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
