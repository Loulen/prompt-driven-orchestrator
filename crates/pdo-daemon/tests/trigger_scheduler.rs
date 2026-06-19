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

/// Create a Trigger with a guard command (and no static input template, so the
/// guard's stdout is the only input source).
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
    assert_eq!(
        resp.status(),
        201,
        "POST /triggers (guarded) should succeed"
    );
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
        .expect("trigger should exist")
}

#[tokio::test]
async fn due_trigger_creates_a_run_with_triggered_by_provenance() {
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
}

#[tokio::test]
async fn overlap_skip_while_previous_run_is_live() {
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
}

#[tokio::test]
async fn missed_slots_are_forward_only_no_backfill() {
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
}

/// The resolved Run input is recorded in the `run_started` event payload; this
/// reads it back so a test can assert what input a guarded fire produced.
async fn run_started_input(daemon: &TestDaemon, run_id: &str) -> String {
    let events: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{}/runs/{}/events", daemon.url(), run_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    events
        .iter()
        .find(|e| e["kind"].as_str() == Some("run_started"))
        .and_then(|e| e["payload"]["input"].as_str())
        .expect("run_started event with an input")
        .to_string()
}

#[tokio::test]
async fn guard_exit_zero_fires_with_stdout_as_input() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Guard exits 0 and prints work to do; its stdout becomes the Run input.
    let trigger =
        create_trigger_with_guard(&daemon, "fixer", "* * * * *", "printf 'issue-42'").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    let runs = list_runs(&daemon).await;
    assert_eq!(runs.len(), 1, "guard exit 0 must fire a Run");
    let run_id = runs[0]["run_id"].as_str().unwrap();
    assert_eq!(
        run_started_input(&daemon, run_id).await,
        "issue-42",
        "the guard stdout must be the Run input"
    );

    let fires = list_fires(&daemon, &trigger_id).await;
    assert_eq!(fires[0]["outcome"].as_str(), Some("fired"));

    cleanup_runs(&daemon).await;
}

#[tokio::test]
async fn concurrent_ticks_fire_a_due_trigger_exactly_once() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // The guard sleeps long enough that a second tick starting mid-guard would
    // also read the trigger as due (next_fire is only recomputed after the
    // guard returns) and double-fire it — the race between the 30 s background
    // loop and the test seam that made `guard_exit_zero_fires_with_stdout_as_input`
    // flake under full-suite load. Ticks must serialize. The yearly cron keeps
    // the recomputed next fire far away, so the second tick can't be
    // legitimately due again. Keep the sleep under the 200 ms guard-timeout
    // override that `guard_timeout_records_guard_error_and_skips` sets
    // process-wide (std::env is shared across parallel tests).
    let trigger = create_trigger_with_guard(
        &daemon,
        "racer",
        "0 0 1 1 *",
        "sleep 0.15; printf 'issue-7'",
    )
    .await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&trigger_id).await;
    tokio::join!(daemon.run_trigger_tick(), daemon.run_trigger_tick());

    let runs = list_runs(&daemon).await;
    assert_eq!(runs.len(), 1, "concurrent ticks must not double-fire");

    let fired = list_fires(&daemon, &trigger_id)
        .await
        .iter()
        .filter(|f| f["outcome"].as_str() == Some("fired"))
        .count();
    assert_eq!(fired, 1, "exactly one 'fired' record");

    cleanup_runs(&daemon).await;
}

#[tokio::test]
async fn guard_exit_nonzero_skips_without_firing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Guard exits non-zero: no work to do, so no Run is created.
    let trigger = create_trigger_with_guard(&daemon, "no-work", "* * * * *", "exit 3").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    assert!(
        list_runs(&daemon).await.is_empty(),
        "a non-zero guard must not fire a Run"
    );
    let fires = list_fires(&daemon, &trigger_id).await;
    assert_eq!(fires[0]["outcome"].as_str(), Some("guard-exit-nonzero"));
}

#[tokio::test]
async fn guard_timeout_records_guard_error_and_skips() {
    // Shrink the guard timeout so a hung guard times out fast in the test.
    std::env::set_var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV, "200");
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // A guard that never returns: the hard timeout classifies it as an error,
    // records `guard-error`, and no Run is created.
    let trigger = create_trigger_with_guard(&daemon, "hung-guard", "* * * * *", "sleep 30").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    assert!(
        list_runs(&daemon).await.is_empty(),
        "a timed-out guard must not fire a Run"
    );
    let fires = list_fires(&daemon, &trigger_id).await;
    assert_eq!(
        fires[0]["outcome"].as_str(),
        Some("guard-error"),
        "a guard timeout must record a guard-error outcome"
    );

    std::env::remove_var(pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV);
}

#[tokio::test]
async fn dangling_pipeline_reference_yields_error_outcome_and_stops_firing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let trigger = create_trigger(&daemon, "audit", "* * * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    // The pipeline is deleted out from under the Trigger (renamed/removed).
    std::fs::remove_file(
        daemon
            .repo_root()
            .join(".pdo")
            .join("pipelines")
            .join(format!("{PIPELINE_NAME}.yaml")),
    )
    .unwrap();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    assert!(
        list_runs(&daemon).await.is_empty(),
        "a dangling pipeline must not fire a Run"
    );
    let fires = list_fires(&daemon, &trigger_id).await;
    assert_eq!(
        fires[0]["outcome"].as_str(),
        Some("error"),
        "a dangling pipeline must record an error outcome"
    );
    // The Trigger stops firing: next_fire is cleared, last_outcome shows error.
    let t = get_trigger(&daemon, &trigger_id).await;
    assert!(
        t["next_fire_at"].is_null(),
        "a dangling-ref Trigger must stop firing (next_fire cleared)"
    );
    assert_eq!(t["last_outcome"].as_str(), Some("error"));
}

#[tokio::test]
async fn dangling_target_repo_reference_yields_error_outcome() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // A target repo that does not exist at fire time (deleted/renamed).
    let body = serde_json::json!({
        "name": "ghost-repo",
        "pipeline_id": PIPELINE_NAME,
        "cron": "* * * * *",
        "input_template": "audit",
        "target_repo": daemon.repo_root().to_string_lossy(),
    });
    let created: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let trigger_id = created["id"].as_str().unwrap().to_string();

    // Remove the git repo so the target_repo no longer validates.
    std::fs::remove_dir_all(daemon.repo_root().join(".git")).unwrap();

    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;

    assert!(
        list_runs(&daemon).await.is_empty(),
        "a dangling target repo must not fire a Run"
    );
    let fires = list_fires(&daemon, &trigger_id).await;
    assert_eq!(fires[0]["outcome"].as_str(), Some("error"));
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

// --- #162: lifecycle management (GET one, PATCH, enable/disable, DELETE) ---

async fn patch_trigger(
    daemon: &TestDaemon,
    trigger_id: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .patch(format!("{}/triggers/{}", daemon.url(), trigger_id))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn get_trigger_one(daemon: &TestDaemon, trigger_id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{}/triggers/{}", daemon.url(), trigger_id))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn get_single_trigger_returns_its_config() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = create_trigger(&daemon, "audit", "0 9 * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    let resp = get_trigger_one(&daemon, &trigger_id).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str(), Some(trigger_id.as_str()));
    assert_eq!(body["name"].as_str(), Some("audit"));
    assert_eq!(body["cron"].as_str(), Some("0 9 * * *"));
}

#[tokio::test]
async fn get_single_trigger_404_when_missing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = get_trigger_one(&daemon, "trg-nope").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn patch_disable_then_enable_pauses_and_resumes_firing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = create_trigger(&daemon, "pausable", "* * * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    // Disable.
    let resp = patch_trigger(
        &daemon,
        &trigger_id,
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(get_trigger(&daemon, &trigger_id).await["enabled"], false);

    // A disabled, forced-due Trigger does not fire.
    daemon.force_trigger_due(&trigger_id).await;
    daemon.run_trigger_tick().await;
    assert!(
        list_runs(&daemon).await.is_empty(),
        "a disabled Trigger must not fire"
    );

    // Re-enable.
    let resp = patch_trigger(&daemon, &trigger_id, serde_json::json!({ "enabled": true })).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(get_trigger(&daemon, &trigger_id).await["enabled"], true);
}

#[tokio::test]
async fn patch_edits_schedule_input_and_overlap_and_recomputes_next_fire() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = create_trigger(&daemon, "editable", "0 9 * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();
    let original_next = trigger["next_fire_at"].as_str().unwrap().to_string();

    let resp = patch_trigger(
        &daemon,
        &trigger_id,
        serde_json::json!({
            "cron": "*/15 * * * *",
            "input_template": "do the new thing",
            "overlap_policy": "allow",
        }),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let after = get_trigger(&daemon, &trigger_id).await;
    assert_eq!(after["cron"].as_str(), Some("*/15 * * * *"));
    assert_eq!(after["input_template"].as_str(), Some("do the new thing"));
    assert_eq!(after["overlap_policy"].as_str(), Some("allow"));
    // Changing the schedule recomputes next_fire_at forward from the new cron.
    let new_next = after["next_fire_at"].as_str().unwrap();
    assert_ne!(
        new_next, original_next,
        "a schedule edit must recompute next_fire_at"
    );
}

#[tokio::test]
async fn patch_rejects_an_invalid_cron() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = create_trigger(&daemon, "audit", "0 9 * * *").await;
    let trigger_id = trigger["id"].as_str().unwrap().to_string();

    let resp = patch_trigger(
        &daemon,
        &trigger_id,
        serde_json::json!({ "cron": "not a cron" }),
    )
    .await;
    assert_eq!(resp.status(), 400);
    // The stored cron is unchanged.
    assert_eq!(get_trigger(&daemon, &trigger_id).await["cron"], "0 9 * * *");
}

#[tokio::test]
async fn patch_missing_trigger_is_404() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = patch_trigger(&daemon, "trg-nope", serde_json::json!({ "enabled": false })).await;
    assert_eq!(resp.status(), 404);
}

// --- #222: timezone fix, panic isolation, health signal ---

async fn get_health(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{}/triggers/health", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// #222 write-side: a created Trigger stores `next_fire_at` as canonical UTC
/// (`…Z`), not a local offset, and it round-trips through GET unchanged. The
/// load-bearing read-side regression (a past-due local-offset row staying due)
/// lives in `trigger_store`'s unit tests, where it can fail pre-fix on any host.
#[tokio::test]
async fn created_trigger_stores_next_fire_in_utc() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = create_trigger(&daemon, "utc-check", "*/15 * * * *").await;
    let next = trigger["next_fire_at"].as_str().expect("a next fire");
    assert!(
        next.ends_with('Z'),
        "next_fire_at must be stored as canonical UTC (…Z), got {next}"
    );
    let fetched = get_trigger(&daemon, trigger["id"].as_str().unwrap()).await;
    assert_eq!(
        fetched["next_fire_at"].as_str(),
        Some(next),
        "next_fire_at must round-trip through GET unchanged"
    );
}

/// #222 ask #1: a tick that panics no longer kills the scheduler. The panic is
/// contained at the supervised `tokio::spawn` boundary, so the driving call
/// returns normally and the next tick keeps firing due Triggers.
#[tokio::test]
async fn a_panicking_tick_does_not_disable_the_scheduler() {
    let daemon = TestDaemon::spawn_with_panic_trigger(seed, "poison")
        .await
        .unwrap();

    let healthy = create_trigger(&daemon, "healthy", "* * * * *").await;
    let healthy_id = healthy["id"].as_str().unwrap().to_string();
    let poison = create_trigger(&daemon, "poison", "* * * * *").await;
    let poison_id = poison["id"].as_str().unwrap().to_string();

    // Tick 1: only `poison` is due (a freshly created `* * * * *` Trigger's next
    // fire is the upcoming whole minute, so `healthy` is not yet due). The poison
    // tick panics — but the supervised boundary contains it, so this call returns
    // NORMALLY. Pre-fix, the panic would unwind the test task right here.
    daemon.force_trigger_due(&poison_id).await;
    daemon.run_trigger_tick().await;
    assert!(
        list_runs(&daemon).await.is_empty(),
        "the panicking tick must not have created a run"
    );

    // Retire the poison pill (an operator disables the bad Trigger) and tick
    // again. This is the payload: a dead scheduler loop could never fire here.
    let resp = patch_trigger(&daemon, &poison_id, serde_json::json!({ "enabled": false })).await;
    assert_eq!(resp.status(), 200);
    daemon.force_trigger_due(&healthy_id).await;
    daemon.run_trigger_tick().await;

    let runs = list_runs(&daemon).await;
    assert_eq!(
        runs.len(),
        1,
        "the scheduler must keep firing after a contained panic"
    );
    assert_eq!(runs[0]["triggered_by"].as_str(), Some(healthy_id.as_str()));
    let fires = list_fires(&daemon, &healthy_id).await;
    assert_eq!(fires[0]["outcome"].as_str(), Some("fired"));

    cleanup_runs(&daemon).await;
}

/// #222 ask #2: `GET /triggers/health` exposes the scheduler's last tick and the
/// configured interval, and the timestamp advances as ticks run.
#[tokio::test]
async fn triggers_health_reports_last_tick_and_advances() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let h = get_health(&daemon).await;
    assert_eq!(
        h["tick_interval_secs"].as_u64(),
        Some(30),
        "health must report the configured tick interval"
    );

    // Drive a tick; last_tick_at becomes non-null.
    daemon.run_trigger_tick().await;
    let t1 = get_health(&daemon).await["last_tick_at"]
        .as_str()
        .expect("last_tick_at set after a tick")
        .to_string();

    // A later tick advances it (canonical-UTC strings compare chronologically;
    // tolerate extra background-loop ticks — the value only moves forward).
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    daemon.run_trigger_tick().await;
    let t2 = get_health(&daemon).await["last_tick_at"]
        .as_str()
        .expect("last_tick_at still set")
        .to_string();

    assert!(
        t2 > t1,
        "last_tick_at must advance across ticks: {t1} then {t2}"
    );
}

fn seed_prompt_required(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
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
