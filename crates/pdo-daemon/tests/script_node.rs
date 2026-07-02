//! Layer 3a — `script` node type (#248 / ADR-0017).
//!
//! A script node runs the author's bash in a tmux session (bash instead of
//! `claude`) and self-signals via `pdo complete` / `pdo fail`. This is the
//! *only* node type that is end-to-end testable in CI with **zero stubbing**:
//! the script IS deterministic bash, so it bypasses the `tmux_cmd_override`
//! test seam entirely (the daemon's default `exec sleep 600` override does not
//! touch it). These tests drive `POST /runs` → poll `GET /runs/{id}` and assert
//! on the real terminal state the bash produced.
//!
//! The tmux session's wrapper calls bare `pdo complete`/`pdo fail`, so the
//! built `pdo` binary must be resolvable on PATH inside the session. The tmux
//! server inherits the daemon (= test process) environment at first spawn, so
//! we prepend `CARGO_BIN_EXE_pdo`'s directory to PATH once before spawning.

mod common;

use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use common::TestDaemon;

const PIPELINE_NAME: &str = "script-cycle";
const NODE_ID: &str = "notify";

/// A `start → script → end` pipeline. The script declares one output port
/// (`out`); its bash body is seeded per-test into the node's prompt slot.
const PIPELINE_YAML: &str = r#"name: script-cycle
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: notify
    name: notify
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: notify, port: in }
  - source: { node: notify, port: out }
    target: { node: end, port: result }
"#;

/// A pipeline where the script declares NO output port (the Discord-ping shape:
/// a pure side effect). `outputs_validator` no-ops for it. `end` is fed straight
/// from `start` so the run can complete without an output from `notify`.
const PIPELINE_YAML_NO_OUTPUT: &str = r#"name: script-noout
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: notify
    name: notify
    type: script
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: notify, port: in }
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

fn ensure_pdo_on_path() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let bin = std::path::Path::new(env!("CARGO_BIN_EXE_pdo"));
        let dir = bin.parent().expect("pdo binary has a parent dir");
        let existing = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), existing));
    });
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!("git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
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

/// Seed the pipeline YAML + the script node's bash body (into its prompt slot).
fn seed_with(yaml: &str, name: &str, body: &str) -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
    let yaml = yaml.to_string();
    let name = name.to_string();
    let body = body.to_string();
    move |repo: &std::path::Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), &yaml)?;
        let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), &body)?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

async fn start_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let body = serde_json::json!({ "pipeline": pipeline, "input": "hello" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Poll `GET /runs/{id}` until `nodes[node_id].status` reaches `expected`, or
/// time out. Returns the final run JSON.
async fn wait_for_node_status(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    expected: &str,
) -> serde_json::Value {
    let deadline = Duration::from_secs(30);
    let started = std::time::Instant::now();
    let mut last = serde_json::Value::Null;
    while started.elapsed() < deadline {
        let run = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        let status = run["nodes"][node_id]["status"].as_str().unwrap_or("");
        if status == expected {
            return run;
        }
        // Terminal-but-unexpected: fail fast rather than spin the full timeout.
        if matches!(status, "failed" | "completed") && status != expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    last
}

#[tokio::test]
async fn script_node_completes_on_exit_zero() {
    ensure_pdo_on_path();
    // Body: write a sentinel (untracked → passes the doc-only-effect clean
    // guard) and the declared output via $PDO_OUTPUT_OUT.
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        echo ok > SENTINEL_SCRIPT\n\
        printf 'hello from a script node\\n' > \"$PDO_OUTPUT_OUT\"\n";
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, body))
        .await
        .unwrap();

    let run_id = start_run(&daemon, PIPELINE_NAME).await;
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "completed").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "completed",
        "script node should complete on exit 0; run was: {run}"
    );

    // The author-written output.md is present with the expected bytes.
    let out = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1/out/output.md");
    let content = std::fs::read_to_string(&out).expect("output.md should exist");
    assert!(content.contains("hello from a script node"), "output bytes: {content}");

    // The side effect landed in the run's shared worktree.
    let sentinel = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/SENTINEL_SCRIPT");
    assert!(sentinel.exists(), "sentinel side-effect should exist at {sentinel:?}");
}

#[tokio::test]
async fn script_node_fails_on_nonzero_exit() {
    ensure_pdo_on_path();
    // A non-zero exit fails the node before any output check, so the declared
    // output port in PIPELINE_YAML is irrelevant here.
    let body = "#!/usr/bin/env bash\nexit 7\n";
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, body))
        .await
        .unwrap();

    let run_id = start_run(&daemon, PIPELINE_NAME).await;
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "failed").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "failed",
        "a non-zero exit must fail the node; run was: {run}"
    );
    let reason = run["nodes"][NODE_ID]["failure_reason"].as_str().unwrap_or("");
    assert!(reason.contains("exited 7"), "reason should name the exit code; got: {reason}");
}

#[tokio::test]
async fn script_node_timeout_exit_code_fails_with_timeout_reason() {
    ensure_pdo_on_path();
    // A body that exits 124 is indistinguishable to the wrapper from a real
    // `timeout` expiry (which also exits 124) — this exercises the exit-code →
    // timeout-reason mapping without waiting out the 60s wall-clock bound.
    let body = "#!/usr/bin/env bash\nexit 124\n";
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, body))
        .await
        .unwrap();

    let run_id = start_run(&daemon, PIPELINE_NAME).await;
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "failed").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "failed", "run was: {run}");
    let reason = run["nodes"][NODE_ID]["failure_reason"].as_str().unwrap_or("");
    assert!(reason.contains("timed out"), "reason should say timed out; got: {reason}");
}

#[tokio::test]
async fn script_node_with_no_output_completes() {
    ensure_pdo_on_path();
    // The Discord-ping shape: a pure side effect, zero declared outputs.
    // `outputs_validator` no-ops (no ports), so exit 0 ⇒ completed.
    let body = "#!/usr/bin/env bash\nset -euo pipefail\necho pinged > PING_SENTINEL\n";
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML_NO_OUTPUT, "script-noout", body))
        .await
        .unwrap();

    let run_id = start_run(&daemon, "script-noout").await;
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "completed").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "completed",
        "a no-output script should complete on exit 0; run was: {run}"
    );
    let sentinel = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/PING_SENTINEL");
    assert!(sentinel.exists(), "side effect should have run at {sentinel:?}");
}

#[tokio::test]
async fn empty_script_body_refuses_launch() {
    ensure_pdo_on_path();
    // An empty body would `bash <empty>` → exit 0 → silent no-op. The launch is
    // refused (400) with no run created, mirroring the dangling-edge refusal.
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, "   \n"))
        .await
        .unwrap();

    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "hello" });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "empty script body must refuse the launch");
    let err = resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        err["error"].as_str().unwrap_or("").contains("empty body"),
        "error should name the empty body; got: {err}"
    );
}

#[tokio::test]
async fn script_node_missing_declared_output_fails_fast() {
    ensure_pdo_on_path();
    // Declares output `out` (PIPELINE_YAML) but writes nothing → output
    // validation finds a missing output. A script has already exited, so there
    // is no agent to nudge: the node must fail-fast, not strand behind a 409.
    let body = "#!/usr/bin/env bash\ntrue\n";
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, body))
        .await
        .unwrap();

    let run_id = start_run(&daemon, PIPELINE_NAME).await;
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "failed").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "failed",
        "missing declared output must fail-fast; run was: {run}"
    );
}
