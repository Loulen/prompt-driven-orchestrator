//! Layer 3a — sandbox tracer bullet (#407, slice D of PRD #403).
//!
//! Drives `POST /runs` against a **real daemon** with a **fake `docker`** (via
//! `docker_cmd_override`) and a tempdir-scoped sandbox home (via
//! `sandbox_home_override`), so no test needs Docker, touches the real `$HOME`,
//! or launches real claude. Asserts the run-advance wiring:
//!   1. a `pure` run projects `sandbox=pure`, prep runs (`create`+`start`), the
//!      node tail is wrapped (`docker exec … pdo-sbx-<run>`), and the run completes;
//!   2. Docker unavailable → `RunFailed`, ZERO host spawn (no `NodeStarted`);
//!   3. an `off` run invokes docker NOT AT ALL (argv log empty) and completes on
//!      the host, byte-for-byte as before;
//!   4. `cleanup_run` removes the container (`rm -f pdo-sbx-<run>`) + purges staging;
//!   5. `boot_recovery` re-ensures a live sandboxed run's container;
//!   6. killing a sandboxed node issues a targeted in-container `docker exec` kill
//!      carrying the session marker.
//!
//! The real end-to-end run (a live container, `pdo complete` from inside it) is
//! the Layer-5 job — a fake `docker exec` cannot run the node's body, so tests
//! that need a terminal state SIMULATE the container's callback by POSTing the
//! node-done endpoint (exactly what `pdo complete` does over HTTP).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant};

use common::TestDaemon;
use tempfile::TempDir;

const NODE_ID: &str = "notify";

/// `start → notify(script, output `out`) → end`, with `end` fed from `notify.out`,
/// so completing `notify` (with its output present) drives the whole run terminal.
const PIPELINE_YAML: &str = r#"name: sbx-cycle
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

fn ensure_pdo_on_path() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let bin = Path::new(env!("CARGO_BIN_EXE_pdo"));
        let dir = bin.parent().expect("pdo binary has a parent dir");
        let existing = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), existing));
    });
}

fn git_init_with_commit(repo: &Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
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

fn seed(body: &str) -> impl FnOnce(&Path) -> anyhow::Result<()> {
    let body = body.to_string();
    move |repo: &Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join("sbx-cycle.yaml"), PIPELINE_YAML)?;
        let prompts_dir = pipelines_dir.join("sbx-cycle.prompts");
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), &body)?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

/// Write a fake `docker` into a test-owned dir and return `(dir, docker_path, log)`.
/// Logs every invocation's argv (one line per arg) to `argv.log`. Canned:
/// `image inspect` → present (exit 0, no build); `container inspect` → ABSENT
/// (exit 1 + sentinel), so `ensure_running` does `create` + `start`; every other
/// subcommand exits 0. `sq` single-quotes the embedded log path.
fn write_fake_docker() -> (TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-docker");
    let log = dir.path().join("argv.log");
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let script = format!(
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$@\" >> {log}\n\
         case \"$1\" in\n\
         image) exit 0 ;;\n\
         container) printf '%s' 'Error: No such container' >&2; exit 1 ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

fn log_text(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// `POST /runs` with an optional `sandbox` mode. Returns the new run id.
async fn start_run(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
    let mut body = serde_json::json!({ "pipeline": "sbx-cycle", "input": "hello" });
    if let Some(mode) = sandbox {
        body["sandbox"] = serde_json::json!(mode);
    }
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

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

async fn wait_until<F>(mut pred: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    pred()
}

async fn wait_run_status(daemon: &TestDaemon, run_id: &str, expected: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = serde_json::Value::Null;
    while Instant::now() < deadline {
        let run = get_run(daemon, run_id).await;
        if run["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    last
}

async fn wait_node_status(daemon: &TestDaemon, run_id: &str, expected: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = serde_json::Value::Null;
    while Instant::now() < deadline {
        let run = get_run(daemon, run_id).await;
        if run["nodes"][NODE_ID]["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    last
}

async fn post_command(daemon: &TestDaemon, run_id: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// Simulate the container writing the node's declared output to the shared mount
/// (host path == container path). Written on the host before the simulated
/// `pdo complete` so node-done's output validation passes.
fn write_node_output(daemon: &TestDaemon, run_id: &str, content: &str) {
    let out = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1/out/output.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, content).unwrap();
}

async fn simulate_node_done(daemon: &TestDaemon, run_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/{NODE_ID}/done", daemon.url()))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "simulated pdo complete should succeed: {}",
        resp.status()
    );
}

// -- Test 1: pure run wires end-to-end ---------------------------------------

#[tokio::test]
async fn pure_run_prepares_wraps_and_completes() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        docker,
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("pure")).await;

    // (a) The mode is projected onto the Run from RunStarted.
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(run["sandbox"], "pure", "run must project sandbox=pure: {run}");

    // (b) Eager prep created + started the container (ensure_ready).
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("create") && t.contains("start")
        })
        .await,
        "prep must create+start the container; log:\n{}",
        log_text(&log)
    );

    // (c) The node's tail was wrapped: a `docker exec … pdo-sbx-<run>` launched it.
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("exec") && t.contains(&format!("pdo-sbx-{run_id}"))
        })
        .await,
        "the node tail must run via `docker exec pdo-sbx-{run_id}`; log:\n{}",
        log_text(&log)
    );

    // (d) The node reaches Running (NodeStarted appended after prep OK). The
    // container would write its output to the shared mount then call `pdo
    // complete`; we simulate both (write the output on the host = same path, then
    // POST node-done) and assert the whole run reaches `completed`.
    let run = wait_node_status(&daemon, &run_id, "running").await;
    assert_eq!(run["nodes"][NODE_ID]["status"], "running", "run: {run}");
    write_node_output(&daemon, &run_id, "hello from the sandbox\n");
    simulate_node_done(&daemon, &run_id).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "pure run must complete: {run}");
}

// -- Test 2: Docker unavailable → RunFailed, no host spawn -------------------

#[tokio::test]
async fn docker_unavailable_fails_run_with_no_host_spawn() {
    ensure_pdo_on_path();
    // A docker binary that does not exist: `ensure_image` hits ErrorKind::NotFound.
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        "/nonexistent/pdo-fake-docker-xyz".to_string(),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("pure")).await;

    let run = wait_run_status(&daemon, &run_id, "failed").await;
    assert_eq!(
        run["status"], "failed",
        "a sandboxed run must fail loud when Docker is unavailable: {run}"
    );
    // ZERO host spawn: the node was never started (no host fallback).
    assert!(
        run["nodes"].get(NODE_ID).is_none()
            || run["nodes"][NODE_ID]["status"] == serde_json::Value::Null,
        "no NodeStarted — the sandboxed node must NOT fall back to a host spawn: {run}"
    );
}

// -- Test 3: off run invokes no docker, completes on the host ----------------

#[tokio::test]
async fn off_run_never_invokes_docker() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    // A real body that writes its declared output and self-signals `pdo complete`
    // on the host (off path). The sentinel is untracked → passes the
    // doc-only-effect clean guard.
    let daemon = TestDaemon::spawn_with_docker_override(
        seed(
            "#!/usr/bin/env bash\nset -euo pipefail\n\
             echo ok > OFF_SENTINEL\n\
             printf 'off output\\n' > \"$PDO_OUTPUT_OUT\"\n",
        ),
        docker,
    )
    .await
    .unwrap();

    // No `sandbox` param → Off → host execution.
    let run_id = start_run(&daemon, None).await;
    let run = wait_run_status(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "off run must complete on host: {run}");
    assert_eq!(run["sandbox"], "off", "default mode is off: {run}");

    // Docker was NEVER invoked on the off parcours.
    assert_eq!(
        log_text(&log),
        "",
        "the `off` path must not invoke docker at all"
    );
}

// -- Test 4: cleanup_run removes the container + purges staging ---------------

#[tokio::test]
async fn cleanup_run_removes_container_and_staging() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        docker,
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("pure")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    // Staging landed under the tempdir home override (hermetic).
    let staging = daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(&run_id);
    assert!(
        wait_until(|| staging.exists()).await,
        "staging dir should exist before cleanup: {staging:?}"
    );

    let resp = post_command(&daemon, &run_id, serde_json::json!({ "kind": "cleanup_run" })).await;
    assert!(resp.status().is_success(), "cleanup_run should archive");
    wait_run_status(&daemon, &run_id, "archived").await;

    // The container was removed and the staging purged.
    assert!(
        log_text(&log).contains(&format!("pdo-sbx-{run_id}")) && log_text(&log).contains("rm"),
        "cleanup must `docker rm -f pdo-sbx-{run_id}`; log:\n{}",
        log_text(&log)
    );
    assert!(!staging.exists(), "cleanup must purge the staging dir: {staging:?}");
}

// -- Test 5: boot_recovery re-ensures a live sandboxed container -------------

#[tokio::test]
async fn boot_recovery_reensures_sandbox_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        docker,
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("pure")).await;
    let count_creates = |log: &Path| log_text(log).lines().filter(|l| *l == "create").count();

    // Wait for the first prep to create+start (so the run is live), targeting
    // this run's container.
    assert!(
        wait_until(|| {
            let t = log_text(&log);
            t.contains("create") && t.contains("start") && t.contains(&format!("pdo-sbx-{run_id}"))
        })
        .await,
        "initial prep must create+start pdo-sbx-{run_id}; log:\n{}",
        log_text(&log)
    );
    let creates_before = count_creates(&log);

    // Boot recovery reconciles the live sandboxed run — re-ensures the container.
    daemon.run_boot_recovery_tick().await;

    assert!(
        count_creates(&log) > creates_before,
        "boot_recovery must re-ensure the container (a fresh create); log:\n{}",
        log_text(&log)
    );
}

// -- Test 6: killing a sandboxed node targets the container ------------------

#[tokio::test]
async fn kill_node_targets_the_container() {
    ensure_pdo_on_path();
    let (_fake_dir, docker, log) = write_fake_docker();
    let daemon = TestDaemon::spawn_with_docker_override(
        seed("#!/usr/bin/env bash\ntrue\n"),
        docker,
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("pure")).await;
    wait_node_status(&daemon, &run_id, "running").await;

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "kill_node", "node_id": NODE_ID, "iter": 1 }),
    )
    .await;
    assert!(resp.status().is_success(), "kill_node should succeed");

    let marker = format!("PDO_SBX_SESSION=pdo-{run_id}-{NODE_ID}-iter-1");
    assert!(
        wait_until(|| log_text(&log).contains(&marker)).await,
        "kill must issue a targeted in-container exec carrying the session marker \
         `{marker}`; log:\n{}",
        log_text(&log)
    );
}
