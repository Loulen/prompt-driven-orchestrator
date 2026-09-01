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

use std::process::Command;
use std::time::Duration;

use crate::common::{ensure_pdo_on_path, TestDaemon};

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
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

/// Seed the pipeline YAML + the script node's bash body (into its prompt slot).
fn seed_with(
    yaml: &str,
    name: &str,
    body: &str,
) -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
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
    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "hello",
        "target_repo": daemon.target_repo(),
    });
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
    // Body: write a sentinel (untracked → passes the shared-worktree clean
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

    let out = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1/out/output.md");
    let content = std::fs::read_to_string(&out).expect("output.md should exist");
    assert!(
        content.contains("hello from a script node"),
        "output bytes: {content}"
    );

    let sentinel = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/SENTINEL_SCRIPT");
    assert!(
        sentinel.exists(),
        "sentinel side-effect should exist at {sentinel:?}"
    );
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
    let reason = run["nodes"][NODE_ID]["failure_reason"]
        .as_str()
        .unwrap_or("");
    assert!(
        reason.contains("exited 7"),
        "reason should name the exit code; got: {reason}"
    );
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
    let reason = run["nodes"][NODE_ID]["failure_reason"]
        .as_str()
        .unwrap_or("");
    assert!(
        reason.contains("timed out"),
        "reason should say timed out; got: {reason}"
    );
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
    assert!(
        sentinel.exists(),
        "side effect should have run at {sentinel:?}"
    );
}

#[tokio::test]
async fn empty_script_body_refuses_launch() {
    ensure_pdo_on_path();
    // An empty body would `bash <empty>` → exit 0 → silent no-op. The launch is
    // refused (400) with no run created, mirroring the dangling-edge refusal.
    let daemon = TestDaemon::spawn(seed_with(PIPELINE_YAML, PIPELINE_NAME, "   \n"))
        .await
        .unwrap();

    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "hello",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "empty script body must refuse the launch"
    );
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
    // ADR-0049: a missing declared output is a runtime give-up, so the node is
    // `Interrupted` (parking the run `AwaitingUser`), NOT `Failed`. It still
    // fails FAST — no strand behind a 409, no live agent to nudge.
    let run = wait_for_node_status(&daemon, &run_id, NODE_ID, "interrupted").await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "interrupted",
        "missing declared output must interrupt fast; run was: {run}"
    );

    // #490: the projection carries WHICH port was missing — the interrupt reducer
    // reads the same nested evidence a `NodeFailed` would, so the red banner is
    // not an empty list.
    assert_eq!(
        run["nodes"][NODE_ID]["missing_outputs"],
        serde_json::json!(["out"]),
        "the interrupt must say which port is missing; run was: {run}"
    );

    // #490 / ADR-0035 §4 / ADR-0049 — THE regression the fix must not introduce.
    // The refusal is a `409` (exit 4), so the tail's `pdo complete || pdo fail`
    // (guarded by `-ne 4`) must NOT run `pdo fail` and double the verdict. And the
    // runtime NEVER appends `RunFailed` on a validation miss (ADR-0049).
    //
    // Let any doubled append land before counting — a passing count measured too
    // early would be a false green.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    let events: Vec<serde_json::Value> =
        reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert!(
        !events.iter().any(|e| e["kind"] == "run_failed"),
        "the runtime never fails the run on a validation miss (ADR-0049). events={events:#?}"
    );
    let interrupts: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["kind"] == "node_interrupted" && e["node_id"] == NODE_ID)
        .collect();
    assert_eq!(
        interrupts.len(),
        1,
        "exactly one node_interrupted; the tail must not double the daemon's \
         verdict. events={events:#?}"
    );
    let reason = interrupts[0]["payload"]["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("failed output validation"),
        "the surviving reason must be the daemon's fail-fast one, got {reason:?}"
    );
    let run: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        run["status"], "awaiting_user",
        "run must park, not fail: {run}"
    );
}
