//! Layer 3a — admission control / global session cap (#159).
//!
//! Proves the daemon-wide cap on concurrent NodeRun sessions: with the cap set
//! to 1 via `MAESTRO_SESSION_CAP`, a node in a second Run that cannot get a slot
//! enters the `waiting` state and is spawned once the first Run's node frees its
//! slot. Manager sessions are exempt by construction — each Run spawns a manager
//! yet the single node still wins the only slot, so a cap of 1 would be
//! impossible to satisfy if managers were counted.
//!
//! Node *state* (running vs waiting) is projected from the event log, so the
//! assertions do not depend on a real tmux server being present; the
//! `MAESTRO_TMUX_CMD_OVERRIDE` just keeps a missing `claude` binary from
//! polluting logs. Single-test file by design — `set_var` mutates process-global
//! state and we don't want a sibling test racing it.

mod common;

use std::time::Duration;

use common::TestDaemon;
use maestro_daemon::{admission::SESSION_CAP_ENV, TMUX_CMD_OVERRIDE_ENV};

const PIPELINE_NAME: &str = "cap-solo";
const NODE_ID: &str = "solo";
const PIPELINE_YAML: &str = r#"name: cap-solo
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: solo
    name: solo
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
    target: { node: solo, port: in }
"#;

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

async fn create_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do the thing" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn node_status(daemon: &TestDaemon, run_id: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}", daemon.url()))
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json["nodes"][NODE_ID]["status"]
        .as_str()
        .map(|s| s.to_string())
}

/// Poll the projected status of `run_id`'s solo node until it equals `want` or
/// the deadline elapses. Returns the last observed status for diagnostics.
async fn wait_for_status(daemon: &TestDaemon, run_id: &str, want: &str) -> Option<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut last = None;
    while std::time::Instant::now() < deadline {
        last = node_status(daemon, run_id).await;
        if last.as_deref() == Some(want) {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    last
}

#[tokio::test]
async fn over_cap_node_waits_then_starts_when_a_slot_frees() {
    // Cap the whole daemon to a single live NodeRun session.
    std::env::set_var(SESSION_CAP_ENV, "1");
    // Avoid needing a real `claude` on PATH; tmux may or may not be present —
    // either way the node *state* is projected from the event log.
    std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");

    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Run 1: its solo node takes the only slot.
    let run1 = create_run(&daemon).await;
    let s1 = wait_for_status(&daemon, &run1, "running").await;
    assert_eq!(
        s1.as_deref(),
        Some("running"),
        "run1 solo node should occupy the single slot"
    );

    // Run 2: no slot left -> its solo node must enter `waiting`, not `running`.
    let run2 = create_run(&daemon).await;
    let s2 = wait_for_status(&daemon, &run2, "waiting").await;
    assert_eq!(
        s2.as_deref(),
        Some("waiting"),
        "run2 solo node should be throttled into `waiting` while the cap is full"
    );

    // Free the slot by stopping run1's node, then run2's node must start.
    let stop = reqwest::Client::new()
        .post(format!("{}/runs/{run1}/nodes/{NODE_ID}/stop", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(stop.status(), 200, "stopping run1's node should succeed");

    let s2_after = wait_for_status(&daemon, &run2, "running").await;
    assert_eq!(
        s2_after.as_deref(),
        Some("running"),
        "run2 solo node should start once run1 freed the slot"
    );

    // Best-effort cleanup of any leaked sleep sessions.
    let socket = daemon.tmux_socket();
    for run in [&run1, &run2] {
        let session = format!("maestro-{run}-{NODE_ID}-iter-1");
        let _ = std::process::Command::new("tmux")
            .args(["-L", &socket, "kill-session", "-t", &session])
            .output();
    }

    std::env::remove_var(SESSION_CAP_ENV);
    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
}
