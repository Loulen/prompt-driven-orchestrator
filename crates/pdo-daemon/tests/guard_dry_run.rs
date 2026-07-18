//! Layer 3a integration tests for the guard dry-run endpoint (#350).
//!
//! Boots a real daemon and drives `POST /triggers/guard/test` — the "Test guard"
//! button's backend — over HTTP. The whole point of this feature is that it runs
//! the guard through the pure `run_guard` seam with **zero side effects**, so the
//! headline assertions here prove the negative: after any dry-run, `GET /runs` is
//! empty and a witness Trigger's fire history and `next_fire_at` are untouched.
//!
//! The timeout path lives in a separate binary (`guard_dry_run_timeout.rs`)
//! because it must set the process-global `PDO_GUARD_TIMEOUT_MS` env var, which
//! would flake sibling tests sharing this process.

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

/// POST the dry-run endpoint with an arbitrary JSON body and return the response.
async fn guard_test(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/triggers/guard/test", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// Create a Trigger with a guard command, returning the created row. Used as a
/// side-effect *witness*: its fire history and `next_fire_at` must be untouched
/// by any dry-run.
async fn create_trigger_with_guard(
    daemon: &TestDaemon,
    name: &str,
    cron: &str,
    guard_command: &str,
) -> serde_json::Value {
    let body = serde_json::json!({
        "name": name,
        "pipeline_id": PIPELINE_NAME,
        "cron": cron,
        "guard_command": guard_command,
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /triggers (guarded) should succeed");
    resp.json().await.unwrap()
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

async fn list_fires(daemon: &TestDaemon, trigger_id: &str) -> Vec<serde_json::Value> {
    reqwest::Client::new()
        .get(format!("{}/triggers/{}/fires", daemon.url(), trigger_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn get_trigger(daemon: &TestDaemon, trigger_id: &str) -> serde_json::Value {
    let triggers: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{}/triggers", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    triggers
        .into_iter()
        .find(|t| t["id"].as_str() == Some(trigger_id))
        .expect("witness trigger should exist")
}

#[tokio::test]
async fn pass_returns_stdout_and_creates_no_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let resp = guard_test(&daemon, serde_json::json!({ "guard_command": "printf hi" })).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"], "pass");
    assert_eq!(body["stdout"], "hi");
    assert_eq!(body["exit_code"], 0);

    // The whole point: a dry-run spawns nothing.
    assert!(
        list_runs(&daemon).await.is_empty(),
        "a guard dry-run must not create any Run"
    );
}

#[tokio::test]
async fn skip_returns_streams_and_exit_code() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let resp = guard_test(
        &daemon,
        serde_json::json!({ "guard_command": "echo err >&2; printf out; exit 3" }),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"], "skip");
    assert_eq!(body["stdout"], "out");
    assert!(
        body["stderr"].as_str().unwrap().contains("err"),
        "stderr should be captured, got {:?}",
        body["stderr"]
    );
    assert_eq!(body["exit_code"], 3);

    assert!(
        list_runs(&daemon).await.is_empty(),
        "a skipping dry-run must not create any Run"
    );
}

#[tokio::test]
async fn target_repo_is_used_as_cwd() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    // A marker only readable by relative name if the guard runs with CWD = repo.
    std::fs::write(daemon.repo_root().join("marker.txt"), "i am here").unwrap();

    let resp = guard_test(
        &daemon,
        serde_json::json!({
            "guard_command": "cat marker.txt",
            "target_repo": daemon.repo_root().to_string_lossy(),
        }),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"], "pass");
    assert_eq!(body["stdout"], "i am here");
}

#[tokio::test]
async fn absent_target_repo_runs_in_repo_root() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    // No target_repo → cwd falls back to the daemon's repo_root, where `seed`
    // committed a `.gitignore`. Reading it by relative name proves the fallback.
    let resp = guard_test(&daemon, serde_json::json!({ "guard_command": "cat .gitignore" })).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"], "pass");
    assert!(
        body["stdout"].as_str().unwrap().contains(".pdo/runs/"),
        "guard should run in repo_root, got {:?}",
        body["stdout"]
    );
}

#[tokio::test]
async fn empty_command_is_400() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = guard_test(&daemon, serde_json::json!({ "guard_command": "   " })).await;
    assert_eq!(resp.status(), 400);
    assert!(list_runs(&daemon).await.is_empty());
}

#[tokio::test]
async fn invalid_target_repo_is_400() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = guard_test(
        &daemon,
        serde_json::json!({
            "guard_command": "echo hi",
            "target_repo": "/no/such/path/pdo-guard-dry-run-xyz",
        }),
    )
    .await;
    assert_eq!(resp.status(), 400);
    assert!(list_runs(&daemon).await.is_empty());
}

#[tokio::test]
async fn leaves_no_fire_row_and_does_not_bump_next_fire() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // A witness Trigger with its own guard. A dry-run of an unrelated (or even
    // identical) command must not touch its audit trail or schedule.
    let trigger =
        create_trigger_with_guard(&daemon, "witness", "0 9 * * *", "gh issue list").await;
    let trigger_id = trigger["id"].as_str().unwrap();

    let fires_before = list_fires(&daemon, trigger_id).await;
    let next_fire_before = get_trigger(&daemon, trigger_id).await["next_fire_at"].clone();

    // Fire off a batch of dry-runs across all three outcomes.
    for cmd in ["printf ok", "exit 4", "echo boom >&2; exit 1"] {
        let resp = guard_test(&daemon, serde_json::json!({ "guard_command": cmd })).await;
        assert_eq!(resp.status(), 200, "dry-run of {cmd:?} should be 200");
    }

    // Zero side effects, proven against both the audit trail and the scheduler.
    assert!(
        list_runs(&daemon).await.is_empty(),
        "dry-runs must not create any Run"
    );
    let fires_after = list_fires(&daemon, trigger_id).await;
    assert_eq!(
        fires_after.len(),
        fires_before.len(),
        "dry-runs must not append a trigger_fires row"
    );
    let next_fire_after = get_trigger(&daemon, trigger_id).await["next_fire_at"].clone();
    assert_eq!(
        next_fire_after, next_fire_before,
        "dry-runs must not recompute next_fire_at"
    );
}
