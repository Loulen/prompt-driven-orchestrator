//! Layer 3a integration tests for the Trigger scheduler (#160).
//!
//! Boots a real daemon, creates Triggers over HTTP, and drives the scheduler a
//! tick at a time via the test seam `DaemonHandle::run_trigger_tick`. Asserts on
//! observable state through the HTTP API (`GET /runs`, `GET /triggers`,
//! `GET /triggers/<id>/fires`) rather than internals.
//!
//! These exercise the effectful path (`create_run_inner`) that unit tests skip.
//! tmux is substituted with `sleep` so the box doesn't need claude; the run is
//! recorded (with `triggered_by`) before any session spawn, so assertions hold
//! whether or not tmux is present.

mod common;

use common::TestDaemon;
use maestro_daemon::TMUX_CMD_OVERRIDE_ENV;

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

async fn create_trigger(daemon: &TestDaemon, name: &str, cron: &str) -> serde_json::Value {
    let body = serde_json::json!({
        "name": name,
        "pipeline_id": PIPELINE_NAME,
        "cron": cron,
        "input_template": "audit the codebase",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /triggers should succeed");
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

#[tokio::test]
async fn due_trigger_creates_a_run_with_triggered_by_provenance() {
    std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let trigger = create_trigger(&daemon, "nightly audit", "* * * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    // Force it due and tick.
    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    // One run exists, carrying the trigger id as provenance.
    let runs = list_runs(&daemon).await;
    assert_eq!(runs.len(), 1, "expected exactly one triggered run");
    assert_eq!(
        runs[0]["triggered_by"].as_str(),
        Some(trigger_id.as_str()),
        "the run must carry triggered_by provenance"
    );

    // The fire is audited as `fired` and links the run.
    let fires: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{}/triggers/{}/fires", daemon.url(), trigger_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0]["outcome"].as_str(), Some("fired"));
    assert_eq!(
        fires[0]["run_id"].as_str(),
        runs[0]["run_id"].as_str(),
        "fire audit row must link the created run"
    );

    cleanup_runs(&daemon).await;
    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
}

#[tokio::test]
async fn overlap_skip_while_previous_run_is_live() {
    std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let trigger = create_trigger(&daemon, "overlapping", "* * * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    // First tick fires a Run (which stays `running`).
    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;
    assert_eq!(list_runs(&daemon).await.len(), 1);

    // Second tick, with the previous Run still live, must skip — no new Run.
    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;
    let runs = list_runs(&daemon).await;
    assert_eq!(
        runs.len(),
        1,
        "overlap policy must skip a second concurrent fire"
    );

    // The skip is audited.
    let fires: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{}/triggers/{}/fires", daemon.url(), trigger_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Newest first: a skipped-overlap on top of the fired row.
    assert_eq!(fires[0]["outcome"].as_str(), Some("skipped-overlap"));
    assert_eq!(fires[1]["outcome"].as_str(), Some("fired"));

    cleanup_runs(&daemon).await;
    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
}

#[tokio::test]
async fn missed_slots_are_forward_only_no_backfill() {
    std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Hourly trigger; force it long-overdue (as if the daemon were off for days).
    let trigger = create_trigger(&daemon, "hourly", "0 * * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    // Exactly one Run is created — the many missed hourly slots are NOT replayed.
    assert_eq!(
        list_runs(&daemon).await.len(),
        1,
        "missed slots must not be backfilled into a flood of runs"
    );

    // next_fire_at is recomputed forward from now (not the original past slot).
    let triggers: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{}/triggers", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let next = triggers
        .iter()
        .find(|t| t["id"].as_str() == Some(trigger_id.as_str()))
        .and_then(|t| t["next_fire_at"].as_str())
        .expect("trigger should have a recomputed next fire");
    assert!(
        next > "2020-01-01T00:00:00.000Z",
        "next fire must be forward of the forced-past slot; got {next}"
    );

    cleanup_runs(&daemon).await;
    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);
}

#[tokio::test]
async fn create_trigger_rejects_prompt_required_pipeline_without_input() {
    let daemon = TestDaemon::spawn(seed_prompt_required).await.unwrap();
    // Pipeline requires a prompt; no guard, no input template → reject.
    let body = serde_json::json!({
        "name": "bad",
        "pipeline_id": "needs-prompt",
        "cron": "* * * * *",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "must reject at creation");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert!(err["error"].as_str().unwrap().contains("requires a prompt"));
}

#[tokio::test]
async fn create_trigger_rejects_invalid_cron() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let body = serde_json::json!({
        "name": "bad cron",
        "pipeline_id": PIPELINE_NAME,
        "cron": "not a cron expr",
        "input_template": "x",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

fn seed_prompt_required(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    // No `prompt_required` key → defaults to true.
    let yaml = r#"name: needs-prompt
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;
    std::fs::write(pipelines_dir.join("needs-prompt.yaml"), yaml)?;
    git_init_with_commit(repo)?;
    Ok(())
}

/// Best-effort: kill any tmux sessions the runs spawned so a `sleep 60` doesn't
/// leak past the test.
async fn cleanup_runs(daemon: &TestDaemon) {
    let socket = daemon.tmux_socket();
    if let Ok(out) = std::process::Command::new("tmux")
        .args(["-L", &socket, "list-sessions", "-F", "#{session_name}"])
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let _ = std::process::Command::new("tmux")
                .args(["-L", &socket, "kill-session", "-t", line])
                .output();
        }
    }
}
