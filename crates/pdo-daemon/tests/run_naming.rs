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

mod common;

use common::TestDaemon;

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
    type: doc-only
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
async fn create_run(daemon_url: &str, body: serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
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
        &daemon.url(),
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
        &daemon.url(),
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
        &daemon.url(),
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "", "name": "My Run" }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert_eq!(
        entry["name"], "My Run",
        "a user-supplied name must be preserved"
    );
}
