//! Layer 3a — proves the `maestro complete` / `maestro fail` CLI commands
//! exit cleanly instead of panicking on tokio runtime shutdown when invoked
//! from inside a tmux NodeRun session.
//!
//! Surfaced while running the run-minimal scenario for #18: the spawn primitive
//! worked, claude wrote the artifact, but `maestro complete` panicked with
//! "Cannot drop a runtime in a context where blocking is not allowed" — the
//! sync subcommands were running inside `#[tokio::main]`'s async context and
//! `reqwest::blocking` could not safely shut down its inner runtime there.
//!
//! These tests spawn the real `maestro` binary in a subprocess (mirroring what
//! claude does inside the tmux session) and assert clean exit codes against a
//! live TestDaemon.

mod common;

use std::process::Command;
use std::time::Duration;

use common::TestDaemon;

const PIPELINE_NAME: &str = "cli-cycle";
const NODE_ID: &str = "solo";
const PIPELINE_YAML: &str = r#"name: cli-cycle
version: "1.0"
nodes:
  - id: solo
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
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
        let out = Command::new("git").args(args).current_dir(repo).output()?;
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

#[tokio::test]
async fn maestro_complete_does_not_panic_and_marks_node_done() {
    // Bypass spawn_tmux_session entirely so the test doesn't need claude/tmux.
    std::env::set_var(maestro_daemon::TMUX_CMD_OVERRIDE_ENV, "true");

    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "hello" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Give the daemon a beat to record node_started, otherwise /done fights
    // an in-flight write.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Sanity: the daemon is reachable from this process via async reqwest.
    reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .expect("daemon should be reachable from test process")
        .error_for_status()
        .expect("/runs should return 2xx");

    let url = daemon.url();
    let run_id_clone = run_id.clone();
    let bin = env!("CARGO_BIN_EXE_maestro");
    // Run the subprocess on a blocking task so the host runtime stays free to
    // serve the daemon's HTTP requests while `maestro complete` blocks on its
    // own reqwest call.
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .arg("complete")
            .env("MAESTRO_RUN_ID", &run_id_clone)
            .env("MAESTRO_NODE_ID", NODE_ID)
            .env("MAESTRO_NODE_ITER", "1")
            .env("MAESTRO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn maestro complete")
    })
    .await
    .expect("blocking task panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stderr.contains("panicked"),
        "maestro complete must not panic. stderr=\n{stderr}\nstdout=\n{stdout}"
    );
    assert!(
        output.status.success(),
        "maestro complete should exit 0 against a live daemon. \
         exit={:?}\nstderr=\n{stderr}\nstdout=\n{stdout}",
        output.status.code()
    );

    // Side-effect: the daemon now treats the node as done.
    let run = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "completed",
        "node should be marked completed; run state was: {run}"
    );

    std::env::remove_var(maestro_daemon::TMUX_CMD_OVERRIDE_ENV);
}

#[tokio::test]
async fn maestro_fail_does_not_panic() {
    // Same panic path through `reqwest::blocking` — covers the `fail` arm too.
    std::env::set_var(maestro_daemon::TMUX_CMD_OVERRIDE_ENV, "true");

    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "x" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let url = daemon.url();
    let run_id_clone = run_id.clone();
    let bin = env!("CARGO_BIN_EXE_maestro");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(["fail", "--reason", "test-induced failure"])
            .env("MAESTRO_RUN_ID", &run_id_clone)
            .env("MAESTRO_NODE_ID", NODE_ID)
            .env("MAESTRO_NODE_ITER", "1")
            .env("MAESTRO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn maestro fail")
    })
    .await
    .expect("blocking task panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "maestro fail must not panic. stderr=\n{stderr}"
    );

    std::env::remove_var(maestro_daemon::TMUX_CMD_OVERRIDE_ENV);
}
