//! Layer 3a — placeholder display name for prompt-less runs (#184).
//!
//! Boots a real TestDaemon over a `prompt_required: false` pipeline, then proves
//! the daemon's naming decision end-to-end through `POST /runs` + `GET /runs`:
//!
//!   - empty input + no name  → the daemon writes a deterministic
//!     `"Untitled run <ts>"` placeholder (the always-on win of #184).
//!   - non-empty input + no name → NO placeholder; the name stays absent so the
//!     Pipeline Manager can derive it from `_input`.
//!
//! The actual manager *rename* is best-effort real-`claude` behaviour and is not
//! assertable here (the harness runs `sleep`, not `claude`); the preamble wording
//! is covered by the pure tests in `prompt_augmenter`.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "naming-test";
const NODE_ID: &str = "worker";

// `prompt_required: false` so a run with empty input is accepted at creation
// (the create handler rejects empty input on a prompt-required pipeline, #158).
const PIPELINE_YAML: &str = r#"name: naming-test
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

const ROLE_PROMPT: &str = "You are a worker. Do the task.\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;

    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), ROLE_PROMPT)?;

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

/// `POST /runs` with the given body, asserting 201, returning the new run id.
async fn create_run(daemon: &TestDaemon, mut body: serde_json::Value) -> String {
    // #470: default the target repo to the daemon's own root, so each call site
    // stays about naming and none of them has to restate the boundary rule.
    if body.get("target_repo").is_none() {
        body["target_repo"] = serde_json::json!(daemon.target_repo());
    }
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed for {body}");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

/// Fetch `GET /runs` and return the entry for `run_id`.
async fn run_entry(daemon_url: &str, run_id: &str) -> serde_json::Value {
    let resp = reqwest::get(format!("{daemon_url}/runs")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    body.into_iter()
        .find(|r| r["run_id"] == run_id)
        .unwrap_or_else(|| panic!("run {run_id} should appear in GET /runs"))
}

/// A prompt-less run (empty input, no name) gets a deterministic placeholder
/// name written by the daemon at spawn — visible immediately in GET /runs.
#[tokio::test]
async fn prompt_less_run_gets_placeholder_name() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "" }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    let name = entry["name"]
        .as_str()
        .expect("prompt-less run must carry a placeholder name in GET /runs");
    assert!(
        name.starts_with("Untitled run "),
        "placeholder name should start with 'Untitled run ', got: {name:?}"
    );
    // The placeholder is derived from the run-id's own timestamp prefix.
    assert_eq!(
        name,
        format!("Untitled run {}", &run_id[..15]),
        "placeholder must match the run-id timestamp"
    );
}

/// A run launched *with* input but no name gets NO placeholder — the name stays
/// absent so the manager can derive it from `_input`. Gating is on the input,
/// not on `prompt_required`.
#[tokio::test]
async fn run_with_input_but_no_name_has_no_placeholder() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing" }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert!(
        entry.get("name").is_none() || entry["name"].is_null(),
        "run with input must NOT get a placeholder name, got: {:?}",
        entry.get("name")
    );
}

/// A user-supplied name is honoured verbatim and not overwritten by a placeholder.
#[tokio::test]
async fn user_named_run_keeps_its_name() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "", "name": "My Run" }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert_eq!(
        entry["name"], "My Run",
        "a user-supplied name must be preserved"
    );
}

// --- #338: configurable auto-naming ------------------------------------------

/// PUT `/settings`, asserting 200.
async fn put_settings(daemon: &TestDaemon, body: serde_json::Value) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "PUT /settings should succeed for {body}"
    );
}

/// #338: an explicit `auto_name:false` with a name keeps that name — the same as
/// the back-compat name-presence path, but now stated by the flag the UI sends.
#[tokio::test]
async fn explicit_auto_name_false_with_name_keeps_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "do a thing",
            "name": "Kept name",
            "auto_name": false,
        }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert_eq!(
        entry["name"], "Kept name",
        "auto_name:false must keep the supplied name even with input present"
    );
}

/// #338: `auto_name:false` with input but NO name gets a stable per-id placeholder —
/// NOT a derived name and NOT an absent one. This is the load-bearing new case: a
/// value the manager is NOT instructed to rename (the Trigger-off situation).
#[tokio::test]
async fn explicit_auto_name_false_without_name_gets_stable_placeholder() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "do a thing",
            "auto_name": false,
        }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    let name = entry["name"]
        .as_str()
        .expect("auto_name:false with no name must carry a stable placeholder, not stay absent");
    assert_eq!(
        name,
        format!("Untitled run {}", &run_id[..15]),
        "auto_name:false must pin a stable placeholder, overriding the input-derivation"
    );
}

/// #338: `auto_name:true` with an empty input yields the placeholder — the pre-#338
/// behaviour, unchanged, expressed explicitly.
#[tokio::test]
async fn explicit_auto_name_true_with_empty_input_gets_placeholder() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "", "auto_name": true }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert_eq!(
        entry["name"].as_str(),
        Some(format!("Untitled run {}", &run_id[..15]).as_str()),
    );
}

/// #338: the instance default is read FRESH at the create chokepoint. With the
/// default flipped OFF (and no explicit flag, no name), a run with input gets a
/// stable placeholder instead of being left unnamed for the manager to derive —
/// and it bites on the very next create, no restart.
#[tokio::test]
async fn instance_default_off_is_read_fresh_for_a_nameless_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Baseline: with the default ON (built-in), a nameless run with input is left
    // unnamed (DeriveFromInput).
    let derived = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing" }),
    )
    .await;
    let entry = run_entry(&daemon.url(), &derived).await;
    assert!(
        entry.get("name").is_none() || entry["name"].is_null(),
        "default ON: a nameless run with input must stay unnamed, got {:?}",
        entry.get("name")
    );

    // Flip the instance default OFF, then create again — no restart.
    put_settings(&daemon, serde_json::json!({ "default_auto_name": false })).await;
    let placeholdered = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing" }),
    )
    .await;
    let entry = run_entry(&daemon.url(), &placeholdered).await;
    assert_eq!(
        entry["name"].as_str(),
        Some(format!("Untitled run {}", &placeholdered[..15]).as_str()),
        "default OFF must pin a placeholder on the very next create (fresh read, no restart)"
    );
}
