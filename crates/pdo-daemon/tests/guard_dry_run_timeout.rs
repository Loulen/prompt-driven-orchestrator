//! Layer 3a integration test for the guard dry-run *timeout* path (#350).
//!
//! Isolated in its own binary because it sets the process-global
//! `PDO_GUARD_TIMEOUT_MS` env var (`guard_runner.rs` reads it via
//! `std::env::var`); a sibling test mutating or reading it concurrently would
//! flake. Every `tests/*.rs` file is a separate process, so this file is the one
//! place that touches that env var. Do NOT add env-insensitive tests here — keep
//! them in `guard_dry_run.rs`.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "auditor";
const PIPELINE_YAML: &str = r#"name: auditor
version: "1.0"
prompt_required: false
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

async fn list_runs(daemon: &TestDaemon) -> Vec<serde_json::Value> {
    reqwest::Client::new()
        .get(format!("{}/runs", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn timeout_returns_error_outcome() {
    // Squeeze the guard bound to 200ms so a `sleep 30` overruns fast. The daemon
    // resolves the timeout `stored → env → default` on each request; a fresh
    // daemon has no stored value, so this env override wins.
    let saved = std::env::var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV).ok();
    std::env::set_var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV, "200");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers/guard/test", daemon.url()))
        .json(&serde_json::json!({ "guard_command": "sleep 30" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"], "error");
    assert!(
        body["detail"].as_str().unwrap().contains("timed out"),
        "expected a timeout detail, got {:?}",
        body["detail"]
    );

    // A timing-out guard still spawns no Run.
    assert!(
        list_runs(&daemon).await.is_empty(),
        "a timed-out dry-run must not create any Run"
    );

    match saved {
        Some(v) => std::env::set_var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV, v),
        None => std::env::remove_var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV),
    }
}
