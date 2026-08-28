//! Layer 3a — the `pdo complete` CLI contract: it exits cleanly instead of
//! panicking, **and** its exit code tells the truth (#490, ADR-0035 §4).
//!
//! `0` granted or legal duplicate · `3` refused, still your turn · `4` refused, the
//! runtime already ruled · `1` breakdown. Those codes are a **public** contract:
//! they live in pipeline authors' bash, and a `script` node's tail branches on the
//! `4` so it does not double a failure the daemon already recorded.
//!
//! The file name is narrower than its scope: it started as the regression for a
//! panic — `reqwest::blocking` cannot drop its inner runtime inside
//! `#[tokio::main]`'s async context — and grew the exit-code matrix.
//!
//! These tests spawn the real `pdo` binary in a subprocess, mirroring what claude
//! does inside the tmux session; an in-process call would not exercise exit codes.

use std::process::Command;
use std::time::Duration;

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "cli-cycle";
const NODE_ID: &str = "solo";
const PIPELINE_YAML: &str = r#"name: cli-cycle
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

#[tokio::test]
async fn pdo_complete_does_not_panic_and_marks_node_done() {
    // Bypass spawn_tmux_session entirely so the test doesn't need claude/tmux:
    // the node session runs `true` (exits immediately) instead of claude.
    let daemon = TestDaemon::spawn_with_override(seed, Some("true".to_string()))
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
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Give the daemon a beat to record node_started, otherwise /done fights
    // an in-flight write.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create the required output file so output validation passes (refs #36).
    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/solo/iter-1/out");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Output\nDone.").unwrap();

    reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .expect("daemon should be reachable from test process")
        .error_for_status()
        .expect("/runs should return 2xx");

    let url = daemon.url();
    let run_id_clone = run_id.clone();
    let bin = env!("CARGO_BIN_EXE_pdo");
    // Run the subprocess on a blocking task so the host runtime stays free to
    // serve the daemon's HTTP requests while `pdo complete` blocks on its
    // own reqwest call.
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .arg("complete")
            .env("PDO_RUN_ID", &run_id_clone)
            .env("PDO_NODE_ID", NODE_ID)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo complete")
    })
    .await
    .expect("blocking task panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stderr.contains("panicked"),
        "pdo complete must not panic. stderr=\n{stderr}\nstdout=\n{stdout}"
    );
    assert!(
        output.status.success(),
        "pdo complete should exit 0 against a live daemon. \
         exit={:?}\nstderr=\n{stderr}\nstdout=\n{stdout}",
        output.status.code()
    );

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
}

/// #433 / ADR-0043: `pdo complete --auto` — the body the injected `Stop` hook
/// runs. It must honour the SAME exit-code contract as a bare `pdo complete`
/// (exit `0` on a granted completion) AND record the completion as **automatic**
/// (`node_auto_completed`), so the log never claims the agent decided.
#[tokio::test]
async fn pdo_complete_auto_exits_0_and_records_auto_completed() {
    let daemon = TestDaemon::spawn_with_override(seed, Some("true".to_string()))
        .await
        .unwrap();
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
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/solo/iter-1/out");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Output\nDone.").unwrap();

    let url = daemon.url();
    let run_id_clone = run_id.clone();
    let bin = env!("CARGO_BIN_EXE_pdo");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(["complete", "--auto"])
            .env("PDO_RUN_ID", &run_id_clone)
            .env("PDO_NODE_ID", NODE_ID)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo complete --auto")
    })
    .await
    .expect("blocking task panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "stderr=\n{stderr}");
    assert_eq!(
        output.status.code(),
        Some(0),
        "a granted auto-completion exits 0 like the manual path; stderr=\n{stderr}"
    );

    let events: Vec<serde_json::Value> =
        reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["kind"].as_str()).collect();
    assert!(
        kinds.contains(&"node_auto_completed"),
        "--auto must record an AUTOMATIC completion; saw {kinds:?}"
    );
    assert!(
        !kinds.contains(&"node_completed"),
        "--auto must not record a plain node_completed; saw {kinds:?}"
    );
}

/// `pdo complete --auto` on a node with its output still missing is a recoverable
/// refusal → exit `3`, exactly like the manual path — the exit the hook's
/// `; exit 0` swallows so a missing-output turn end never wedges the node.
#[tokio::test]
async fn pdo_complete_auto_exits_3_on_a_recoverable_refusal() {
    let (daemon, run_id) = daemon_with_run().await;
    let url = daemon.url();
    let run_id_clone = run_id.clone();
    let bin = env!("CARGO_BIN_EXE_pdo");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(["complete", "--auto"])
            .env("PDO_RUN_ID", &run_id_clone)
            .env("PDO_NODE_ID", NODE_ID)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo complete --auto")
    })
    .await
    .expect("blocking task panicked");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "stderr=\n{stderr}");
    assert!(stderr.contains("REFUSED"), "stderr=\n{stderr}");
}

/// Boot a daemon + a run, and hand back everything the CLI needs. The tmux override
/// is `true`, so the node session exits instantly and neither claude nor a live tmux
/// is required — the trick that makes the whole exit-code matrix testable in CI.
async fn daemon_with_run() -> (TestDaemon, String) {
    let daemon = TestDaemon::spawn_with_override(seed, Some("true".to_string()))
        .await
        .unwrap();
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "hello",
        // #470 / ADR-0033: required at the create boundary.
        "target_repo": daemon.target_repo(),
    });
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
    tokio::time::sleep(Duration::from_millis(300)).await;
    (daemon, run_id)
}

/// Run the real binary against `url`. On a **blocking** task so the host runtime
/// stays free to serve the request `pdo complete` is blocking on.
async fn run_pdo_complete(url: &str, run_id: &str) -> (Option<i32>, String) {
    let url = url.to_string();
    let run_id = run_id.to_string();
    let bin = env!("CARGO_BIN_EXE_pdo");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .arg("complete")
            .env("PDO_RUN_ID", &run_id)
            .env("PDO_NODE_ID", NODE_ID)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo complete")
    })
    .await
    .expect("blocking task panicked");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    // The refusal goes to stderr, never stdout — a property `run_complete` already
    // had and must keep.
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "pdo complete must write nothing to stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !stderr.contains("panicked"),
        "pdo complete panicked: {stderr}"
    );
    (output.status.code(), stderr)
}

fn write_output_artifact(daemon: &TestDaemon, run_id: &str, body: &str) {
    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts/solo/iter-1/out");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), body).unwrap();
}

/// Exit `3` — refused, and the node is still yours. Nothing terminal is recorded, so
/// the right next move is to write the artefact and call again; `pdo fail` would be
/// wrong. The stderr has to say so, because the agent reading it is the consumer this
/// whole issue is about.
#[tokio::test]
async fn pdo_complete_exits_3_on_a_recoverable_refusal() {
    let (daemon, run_id) = daemon_with_run().await;
    // No artefact written: `missing_outputs`.
    let (code, stderr) = run_pdo_complete(&daemon.url(), &run_id).await;
    assert_eq!(code, Some(3), "stderr=\n{stderr}");
    assert!(stderr.contains("REFUSED"), "stderr=\n{stderr}");
    assert!(
        stderr.contains("out"),
        "must name the missing port: {stderr}"
    );
    assert!(
        stderr.contains("still your turn"),
        "must say the node is still the caller's: {stderr}"
    );
    assert!(
        stderr.contains("Do NOT run `pdo fail`"),
        "must warn against doubling: {stderr}"
    );

    // And it really is still alive — which is what makes `3` rather than `4` correct.
    let run = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_ne!(run["nodes"][NODE_ID]["status"], "failed", "{run}");
}

/// Exit `4` — refused, and the runtime has already ruled. Driving the node failed
/// **with `auto_fail` opted in** drives the whole run terminal (ADR-0049: a plain
/// `pdo fail` now parks the run `AwaitingUser` for a human to confirm, so it is no
/// longer a terminal refusal), so the completion guard answers "resume the run
/// first": the refusal that, before #490, answered `200` and printed
/// "marked complete." on a dead run.
#[tokio::test]
async fn pdo_complete_exits_4_on_a_terminal_refusal() {
    let daemon = TestDaemon::spawn_with_override(seed, Some("true".to_string()))
        .await
        .unwrap();
    // ADR-0049 / AC5: `auto_fail: true` makes the agent `pdo fail` terminalise
    // the run to `Failed` directly, reproducing the pre-résilience terminal state
    // this exit-code contract needs.
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "hello",
        "target_repo": daemon.target_repo(),
        "auto_fail": true,
    });
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
    tokio::time::sleep(Duration::from_millis(300)).await;
    write_output_artifact(&daemon, &run_id, "# Output\nDone.");

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/fail",
            daemon.url()
        ))
        .json(&serde_json::json!({ "reason": "driven terminal on purpose", "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    tokio::time::sleep(Duration::from_millis(400)).await;

    let (code, stderr) = run_pdo_complete(&daemon.url(), &run_id).await;
    assert_eq!(code, Some(4), "stderr=\n{stderr}");
    assert!(stderr.contains("REFUSED"), "stderr=\n{stderr}");
    assert!(
        stderr.contains("Do NOT run `pdo fail`"),
        "the whole point of the 4: {stderr}"
    );
    assert!(
        stderr.contains("already recorded"),
        "must say the runtime already ruled: {stderr}"
    );
}

/// Exit `0` on a **legal duplicate**. Non-negotiable: a puzzled agent that re-runs
/// `pdo complete` must not read "refused" and then chain `pdo fail` — it would kill a
/// run that had just succeeded, and on a `script` node the tail would do it
/// unprompted.
#[tokio::test]
async fn pdo_complete_exits_0_on_a_legal_duplicate() {
    let (daemon, run_id) = daemon_with_run().await;
    write_output_artifact(&daemon, &run_id, "# Output\nDone.");

    let (first, stderr) = run_pdo_complete(&daemon.url(), &run_id).await;
    assert_eq!(first, Some(0), "first call should be granted: {stderr}");
    tokio::time::sleep(Duration::from_millis(400)).await;

    let (second, stderr) = run_pdo_complete(&daemon.url(), &run_id).await;
    assert_eq!(second, Some(0), "a legal duplicate is a success: {stderr}");
    assert!(
        stderr.contains("already complete") || stderr.contains("nothing to do"),
        "must say so truthfully rather than claim a fresh completion: {stderr}"
    );
    assert!(!stderr.contains("REFUSED"), "{stderr}");
}

/// Exit `1` — no verdict at all. The **only** arm where `pdo fail` is the right
/// escalation, and the stderr is the only place that says so.
#[tokio::test]
async fn pdo_complete_exits_1_when_the_daemon_is_unreachable() {
    // A port nothing listens on. No daemon involved, so no run either.
    let (code, stderr) = run_pdo_complete("http://127.0.0.1:1", "no-such-run").await;
    assert_eq!(code, Some(1), "stderr=\n{stderr}");
    assert!(stderr.contains("failed to reach daemon"), "{stderr}");
    assert!(
        stderr.contains("pdo fail"),
        "the one case where signalling failure is right: {stderr}"
    );
}

/// Three lines that kill the laziest possible implementation: returning the HTTP
/// status as the process exit code. `409 % 256 == 153`, and a `u8` exit code cannot
/// express `409` at all — so a status passthrough would collide with nothing
/// meaningful and silently break the tail's `-ne 4` test.
#[tokio::test]
async fn pdo_complete_exit_code_is_not_the_http_status() {
    let (daemon, run_id) = daemon_with_run().await;
    let (code, stderr) = run_pdo_complete(&daemon.url(), &run_id).await;
    assert_eq!(code, Some(3), "stderr=\n{stderr}");
    assert_ne!(code, Some(409 % 256), "the exit code is not the status");
    assert_ne!(code, Some(409), "an exit code cannot even hold 409");
}

#[tokio::test]
async fn pdo_fail_does_not_panic() {
    // Same panic path through `reqwest::blocking` — covers the `fail` arm too.
    let daemon = TestDaemon::spawn_with_override(seed, Some("true".to_string()))
        .await
        .unwrap();

    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "x",
        "target_repo": daemon.target_repo(),
    });
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
    let bin = env!("CARGO_BIN_EXE_pdo");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(["fail", "--reason", "test-induced failure"])
            .env("PDO_RUN_ID", &run_id_clone)
            .env("PDO_NODE_ID", NODE_ID)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo fail")
    })
    .await
    .expect("blocking task panicked");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "pdo fail must not panic. stderr=\n{stderr}"
    );
}
