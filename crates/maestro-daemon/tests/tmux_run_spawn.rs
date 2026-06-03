//! Layer 3a — proves Bug A (#18) is fixed: `spawn_tmux_session` produces a
//! script that invokes `claude --dangerously-skip-permissions` and keeps the
//! tmux session alive until that process exits.
//!
//! The lifecycle test substitutes `claude` with `sleep 60` via
//! `MAESTRO_TMUX_CMD_OVERRIDE` so the test box doesn't actually need claude
//! on PATH. Single-test file by design — `set_var` mutates process-global
//! state and we don't want a sibling test racing it.

mod common;

use std::time::Duration;

use common::TestDaemon;
use maestro_daemon::{build_tmux_script, TMUX_CMD_OVERRIDE_ENV};

const PIPELINE_NAME: &str = "minimal-tmux";
const NODE_ID: &str = "solo";
const PIPELINE_YAML: &str = r#"name: minimal-tmux
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

    // git worktree add (used by create_run) requires a real repo with a HEAD.
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

#[test]
fn build_tmux_script_uses_exec_bash_and_invokes_claude() {
    // Acceptance: the constructed command string contains
    // `claude --dangerously-skip-permissions` and the `exec bash -c` shape.
    // We make sure the override env is unset so we get the production tail.
    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);

    let prompt_path = std::path::Path::new("/tmp/maestro-test/solo-iter-1.md");
    let script = build_tmux_script("run-abc", "solo", 1, 5172, prompt_path);

    assert!(
        script.starts_with("exec bash -c "),
        "script should start with `exec bash -c `; got: {script}"
    );
    assert!(
        script.contains("exec claude --dangerously-skip-permissions"),
        "script must invoke claude with --dangerously-skip-permissions; got: {script}"
    );
    // The script is wrapped in `bash -c '...'`, so single quotes around the
    // embedded prompt path get rewritten as `'\''`. We just need to see that
    // the path appears inside a `cat …` command substitution.
    assert!(
        script.contains("$(cat ") && script.contains("/tmp/maestro-test/solo-iter-1.md"),
        "script must `cat` the prompt file inside command substitution; got: {script}"
    );
    assert!(
        script.contains("export MAESTRO_RUN_ID=") && script.contains("run-abc"),
        "script must export MAESTRO_RUN_ID; got: {script}"
    );
    assert!(
        script.contains("export MAESTRO_DAEMON_URL=") && script.contains("http://localhost:5172"),
        "script must export MAESTRO_DAEMON_URL; got: {script}"
    );
}

#[tokio::test]
async fn tmux_session_alive_after_run_spawn() {
    // tmux must be present for this test to be meaningful.
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping tmux_session_alive_after_run_spawn");
        return;
    }

    // Substitute claude → sleep 60. Set BEFORE spawning the daemon so
    // `build_tmux_script` reads the override at run-creation time.
    std::env::set_var(TMUX_CMD_OVERRIDE_ENV, "exec sleep 60");

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    // The daemon spawns sessions on its own scoped tmux socket (`tmux -L`),
    // not the default server — inspect/kill through the same socket.
    let socket = daemon.tmux_socket();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed");
    let resp_json: serde_json::Value = resp.json().await.unwrap();
    let run_id = resp_json["run_id"].as_str().unwrap().to_string();

    let session = format!("maestro-{run_id}-{NODE_ID}-iter-1");

    // Poll for up to 5s — spawn_node runs async work (worktree, prompt write,
    // tmux exec) before the session appears.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut alive = false;
    while std::time::Instant::now() < deadline {
        if tmux_has_session(&socket, &session) {
            alive = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Always best-effort kill regardless of outcome so a flake doesn't leak
    // a sleep 60 process.
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();

    std::env::remove_var(TMUX_CMD_OVERRIDE_ENV);

    assert!(
        alive,
        "expected tmux session `{session}` to exist within 5s of POST /runs"
    );
}

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmux_has_session(socket: &str, session: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", socket, "has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
