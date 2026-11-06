//! Layer 3a — Run naming (#184, #338, #1039).
//!
//! Boots a real TestDaemon over a `prompt_required: false` pipeline, then proves
//! the daemon's naming decision end-to-end through `POST /runs` + `GET /runs`:
//!
//!   - no name → the daemon ALWAYS writes a deterministic `"Untitled run <ts>"`
//!     placeholder, with or without input (#1039: a Run always has a name);
//!   - auto-naming on → the Run freezes its *Consigne de nommage* (Pipeline →
//!     instance → built-in) and its agent node's prompt asks it to rename the
//!     Run with `if_placeholder` — whether or not a manager runs.
//!
//! The actual agent *rename* is real-`claude` behaviour and is not assertable here
//! (the harness runs `sleep`, not `claude`); the conditional rename itself is
//! covered by the in-crate command tests.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "naming-test";
const NODE_ID: &str = "worker";

// `prompt_required: false` so a run with empty input is accepted at creation
// (the create handler rejects empty input on a prompt-required pipeline, #158).
const PIPELINE_YAML: &str = r#"name: naming-test
version: "1.0"
prompt_required: false
naming_instruction: "HITL #<issue number>"
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

/// The same pipeline WITHOUT a naming instruction, so the instance tier and the
/// built-in default can speak.
const PLAIN_PIPELINE_NAME: &str = "naming-plain";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    let plain_yaml = PIPELINE_YAML
        .replace("name: naming-test", &format!("name: {PLAIN_PIPELINE_NAME}"))
        .replace("naming_instruction: \"HITL #<issue number>\"\n", "");
    for (name, yaml) in [
        (PIPELINE_NAME, PIPELINE_YAML.to_string()),
        (PLAIN_PIPELINE_NAME, plain_yaml),
    ] {
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml)?;
        let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), ROLE_PROMPT)?;
    }

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

/// The projected Run state (`GET /runs/<id>`) — where the frozen
/// `naming_instruction` is observable.
async fn run_state(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// The augmented prompt the worker agent was spawned with, then its session
/// killed (the harness runs `sleep`).
async fn worker_prompt(daemon: &TestDaemon, run_id: &str) -> String {
    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/{NODE_ID}/prompt?iter=1",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200, "the worker must have spawned");
    let body = resp.text().await.unwrap();
    let _ = std::process::Command::new("tmux")
        .args([
            "kill-session",
            "-t",
            &format!("pdo-{run_id}-{NODE_ID}-iter-1"),
        ])
        .output();
    body
}

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

/// #1039 — the root bug: a run launched *with* input, no name, auto-naming on
/// (the default) and the manager off (the default) used to stay nameless. It now
/// carries the placeholder from its creation, and its entry agent — not the
/// manager — is told to rename it following the Pipeline's naming instruction.
#[tokio::test]
async fn run_with_input_gets_placeholder_and_its_entry_agent_is_told_to_name_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing" }),
    )
    .await;

    let entry = run_entry(&daemon.url(), &run_id).await;
    assert_eq!(
        entry["name"].as_str(),
        Some(format!("Untitled run {}", &run_id[..15]).as_str()),
        "a Run always has a name: the placeholder, until its agent renames it"
    );
    assert_eq!(
        run_state(&daemon, &run_id).await["naming_instruction"],
        "HITL #<issue number>",
        "the Pipeline's naming instruction is frozen into the Run"
    );

    let prompt = worker_prompt(&daemon, &run_id).await;
    assert!(prompt.contains("## Name this run"), "{prompt}");
    assert!(prompt.contains("> HITL #<issue number>"), "{prompt}");
    assert!(
        prompt.contains(&format!("/runs/{run_id}/commands")),
        "{prompt}"
    );
    assert!(prompt.contains(r#""if_placeholder":true"#), "{prompt}");
}

/// #1039: auto-naming off ⇒ no agent is asked to rename, and nothing is frozen.
#[tokio::test]
async fn auto_name_off_asks_no_agent_to_name_the_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let run_id = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing", "auto_name": false }),
    )
    .await;

    assert!(run_state(&daemon, &run_id).await["naming_instruction"].is_null());
    let prompt = worker_prompt(&daemon, &run_id).await;
    assert!(!prompt.contains("## Name this run"), "{prompt}");
    assert!(!prompt.contains("rename_run"), "{prompt}");
}

/// #1039: Pipeline → instance → built-in. A pipeline without an instruction takes
/// the instance's, read fresh at the create; a pipeline with one keeps its own.
#[tokio::test]
async fn naming_instruction_falls_back_to_the_instance_then_the_builtin() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let builtin = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PLAIN_PIPELINE_NAME, "input": "x" }),
    )
    .await;
    let frozen = run_state(&daemon, &builtin).await["naming_instruction"].clone();
    assert!(
        frozen.as_str().is_some_and(|s| s.contains("2–5 words")),
        "no Pipeline nor instance instruction: the built-in one, got {frozen}"
    );
    let _ = worker_prompt(&daemon, &builtin).await;

    put_settings(
        &daemon,
        serde_json::json!({ "default_naming_instruction": "US #<n> - <Trello column>" }),
    )
    .await;
    let instance = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PLAIN_PIPELINE_NAME, "input": "x" }),
    )
    .await;
    assert_eq!(
        run_state(&daemon, &instance).await["naming_instruction"],
        "US #<n> - <Trello column>"
    );
    let _ = worker_prompt(&daemon, &instance).await;

    let pipeline = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "x" }),
    )
    .await;
    assert_eq!(
        run_state(&daemon, &pipeline).await["naming_instruction"],
        "HITL #<issue number>",
        "the Pipeline's own instruction wins over the instance's"
    );
    let _ = worker_prompt(&daemon, &pipeline).await;
}

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
/// value no agent is instructed to rename (the Trigger-off situation).
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
/// default flipped OFF (and no explicit flag, no name), a run with input keeps its
/// stable placeholder and no agent is asked to rename it — and it bites on the
/// very next create, no restart.
#[tokio::test]
async fn instance_default_off_is_read_fresh_for_a_nameless_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Baseline: with the default ON (built-in), a nameless run with input carries
    // the placeholder AND the naming instruction its agent will follow.
    let derived = create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "do a thing" }),
    )
    .await;
    assert!(
        !run_state(&daemon, &derived).await["naming_instruction"].is_null(),
        "default ON: the run must freeze a naming instruction"
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
    assert!(
        run_state(&daemon, &placeholdered).await["naming_instruction"].is_null(),
        "default OFF: no agent is asked to rename"
    );
}
