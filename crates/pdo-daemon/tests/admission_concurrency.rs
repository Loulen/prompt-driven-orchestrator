//! Layer 3a — atomic admission under concurrency (#213 AC4).
//!
//! The session cap is enforced as an atomic check-and-reserve: even when many
//! runs are created back-to-back (so their entry nodes race to spawn), the
//! number of nodes that ever hold a live session at once never exceeds the cap.
//! Without the admission lock, concurrent spawns all observe the same free slot
//! and overshoot.
//!
//! Single-test file by design — the cap is set via the process-global
//! `PDO_SESSION_CAP`, so a sibling test in the same binary must not race it.

mod common;

use std::time::Duration;

use common::TestDaemon;
use pdo_daemon::admission::SESSION_CAP_ENV;

const CAP: usize = 2;
const RUNS: usize = 8;

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

async fn create_run(daemon_url: String) -> Option<String> {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "go" });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json["run_id"].as_str().map(String::from)
}

/// Count nodes currently projected as `running` across all runs (each holds a
/// live session / an admission slot).
async fn count_running_nodes(daemon_url: &str, run_ids: &[String]) -> usize {
    let client = reqwest::Client::new();
    let mut running = 0;
    for run_id in run_ids {
        if let Ok(resp) = client
            .get(format!("{daemon_url}/runs/{run_id}"))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json["nodes"][NODE_ID]["status"].as_str() == Some("running") {
                    running += 1;
                }
            }
        }
    }
    running
}

#[tokio::test]
async fn concurrent_spawns_never_exceed_the_cap() {
    std::env::set_var(SESSION_CAP_ENV, CAP.to_string());

    // `TestDaemon::spawn` seeds a harmless `sleep` override per-daemon, so an
    // admitted node keeps holding its slot (never exits) for the duration.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let url = daemon.url();

    // Fire all run creations concurrently so their entry nodes race to spawn.
    let mut handles = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let url = url.clone();
        handles.push(tokio::spawn(async move { create_run(url).await }));
    }
    let mut run_ids = Vec::with_capacity(RUNS);
    for h in handles {
        if let Ok(Some(id)) = h.await {
            run_ids.push(id);
        }
    }
    assert_eq!(run_ids.len(), RUNS, "all runs should be created");

    // Sample the live-session count repeatedly while the system settles. The
    // invariant must hold at every observation: never more than CAP running.
    let mut max_running = 0;
    for _ in 0..20 {
        let running = count_running_nodes(&url, &run_ids).await;
        max_running = max_running.max(running);
        assert!(
            running <= CAP,
            "running node count {running} must never exceed the cap {CAP}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Exactly CAP slots should be in use (the rest are throttled to waiting),
    // proving admission both bounds AND fills the cap.
    assert_eq!(
        max_running, CAP,
        "the cap should be fully used: {CAP} nodes running, the rest waiting"
    );

    // Best-effort cleanup of leaked sleep sessions.
    let socket = daemon.tmux_socket();
    for run in &run_ids {
        let session = format!("pdo-{run}-{NODE_ID}-iter-1");
        let _ = std::process::Command::new("tmux")
            .args(["-L", &socket, "kill-session", "-t", &session])
            .output();
    }

    std::env::remove_var(SESSION_CAP_ENV);
}
