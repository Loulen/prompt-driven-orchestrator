//! Layer 3a — tmux lifecycle tests for issue #23.
//!
//! Tests:
//! 1. Reaper kills sessions for NodeRuns completed > TTL ago.
//! 2. Orphan sweep at boot kills pre-existing stale maestro-* sessions.
//! 3. Dead-session re-spawn: kill a session, hit /pane, assert fresh session.

mod common;

use std::time::Duration;

use common::TestDaemon;
use maestro_daemon::tmux_session_manager;

const PIPELINE_NAME: &str = "lifecycle-test";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: lifecycle-test
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
edges: []
"#;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmux_has_session(session: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn create_fake_tmux_session(name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "sleep", "300"])
        .output();
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
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
    std::fs::write(repo.join(".gitignore"), ".maestro/runs/\n")?;
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

async fn wait_for_session(session: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tmux_has_session(session) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn wait_for_session_gone(session: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !tmux_has_session(session) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Layer 3a: After node completion, the reaper kills the session once
/// the TTL expires. Uses fast TTL (2s) and reaper interval (1s).
#[tokio::test]
async fn reaper_kills_completed_session_after_ttl() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    std::env::set_var(
        tmux_session_manager::TMUX_CMD_OVERRIDE_ENV,
        "exec sleep 300",
    );
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "2");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "1");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&session, Duration::from_secs(5)).await,
        "session should appear after POST /runs"
    );

    // Complete the node — session stays alive per #23
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

    // Session should still be alive right after completion
    assert!(
        tmux_has_session(&session),
        "session should survive node_done (stays for preview)"
    );

    // Wait for reaper to kill it (TTL=2s + interval=1s ≈ 3-4s)
    assert!(
        wait_for_session_gone(&session, Duration::from_secs(10)).await,
        "reaper should kill session after TTL expires"
    );

    // Clean up env
    std::env::remove_var(tmux_session_manager::TMUX_CMD_OVERRIDE_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}

/// Layer 3a: At daemon boot, pre-existing orphan maestro-* sessions get swept.
#[tokio::test]
async fn orphan_sweep_at_boot_kills_stale_session() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // Create a fake maestro session before starting the daemon.
    // This simulates a leftover from a crashed run.
    let orphan_session = "maestro-20260101-120000-aaaaaaa-orphan-iter-1";
    create_fake_tmux_session(orphan_session);
    assert!(
        tmux_has_session(orphan_session),
        "pre-condition: fake session should exist"
    );

    std::env::set_var(
        tmux_session_manager::TMUX_CMD_OVERRIDE_ENV,
        "exec sleep 300",
    );
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "0");

    // Boot the daemon — orphan sweep runs at startup
    let _daemon = TestDaemon::spawn(seed).await.unwrap();

    // Give the sweep a moment to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        !tmux_has_session(orphan_session),
        "orphan session should be killed at boot (run absent from event log)"
    );

    std::env::remove_var(tmux_session_manager::TMUX_CMD_OVERRIDE_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
}

/// Layer 3a: Kill a session manually, hit /pane, assert a fresh session appears.
#[tokio::test]
async fn dead_session_respawn_via_pane_endpoint() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    std::env::set_var(
        tmux_session_manager::TMUX_CMD_OVERRIDE_ENV,
        "exec sleep 300",
    );
    // Long TTL so the reaper doesn't interfere
    std::env::set_var(tmux_session_manager::REAPER_TTL_SECS_ENV, "3600");
    std::env::set_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV, "3600");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);

    assert!(
        wait_for_session(&session, Duration::from_secs(5)).await,
        "session should appear after POST /runs"
    );

    // Kill the session manually
    tmux_session_manager::kill(&session);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !tmux_has_session(&session),
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
        tmux_has_session(&session),
        "session should be re-spawned after /pane request"
    );

    // Clean up
    tmux_session_manager::kill(&session);
    std::env::remove_var(tmux_session_manager::TMUX_CMD_OVERRIDE_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_TTL_SECS_ENV);
    std::env::remove_var(tmux_session_manager::REAPER_INTERVAL_SECS_ENV);
}
