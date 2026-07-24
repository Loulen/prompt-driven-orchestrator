//! Layer 3a — sandbox observability (#408, slice E of PRD #403).
//!
//! Drives `POST /runs` against a **real daemon** with a **fake `docker`** and a
//! tempdir-scoped sandbox home, so no test needs real Docker, touches the real
//! `$HOME`, or launches real claude. Proves the `transcripts_root` seam + the
//! `merge_back` wiring end-to-end (ADR-0004 règle d'or: an AC is closed at layer
//! ≥3, not with fake-probe unit tests alone):
//!   1. cost reads the STAGED home while a sandboxed Run is live (not `~/.claude`);
//!   2. stale-detection reads the staged home while a sandboxed Run is live;
//!   3. the terminal transition merges the staged transcripts into
//!      `~/.claude/projects/` at the standard encoded dirname;
//!   4. the second merge at `cleanup_run` is idempotent (byte-identical, one file);
//!   5. resuming a sandboxed Run re-arms its container (ensure_ready) → 200;
//!   6. resuming a sandboxed Run with Docker unavailable fails loud (500), never a
//!      silent host fallback.
//!
//! For the LIVE-run paths (1, 2) the fake `docker exec` must keep the node's
//! session alive — the default docker-override harness pins the tail to
//! `exec true`, which collapses the session before the sweep can read the mtime
//! (plan #408 P4). So [`write_live_docker`] actually *runs* the exec'd command
//! (`sleep 600`) via a `sleep`-friendly tail, and the run uses
//! [`common::TestDaemon::spawn_with_docker_and_tmux_override`] with `exec sleep 600`.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant, SystemTime};

use common::TestDaemon;
use pdo_daemon::stale_detector::encode_working_dir;
use tempfile::TempDir;

const NODE_ID: &str = "worker";

/// `start → worker(doc-only, in→out) → end`. A doc-only node uses the agent tail
/// so `tmux_cmd_override` (`exec sleep 600`) is honoured — the session stays
/// alive for the live-run observability paths. Completing `worker` (with its
/// `out` present) drives the whole run terminal.
const PIPELINE_YAML: &str = r#"name: sbx-obs
version: "1.0"
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
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
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

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_init_with_commit(repo: &Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {args:?} failed: {}",
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

fn seed(repo: &Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(pipelines_dir.join("sbx-obs.yaml"), PIPELINE_YAML)?;
    // Preserve real HOME for git's global config while the daemon's sandbox home
    // is overridden to the tempdir (git needs a usable global config to make
    // worktrees). Harmless if HOME is unset.
    git_init_with_commit(repo)?;
    Ok(())
}

/// A fake `docker` whose `exec` **actually runs** the exec'd command, so a
/// sandboxed node's `docker exec … bash -lc 'exec sleep 600'` keeps its tmux
/// session alive (needed for the live-run paths). `image inspect` → present,
/// `container inspect` → running (`true`), so `ensure_ready` never builds/creates.
/// Logs every argv (one line per arg) to `argv.log`.
fn write_live_docker() -> (TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-docker");
    let log = dir.path().join("argv.log");
    let sq = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    // On `exec`, shift past every flag up to AND including the `pdo-sbx-<run>`
    // container name, then exec the remaining argv (`bash -lc '<tail>'`). No arg
    // before the container name starts with `pdo-sbx-` (the session marker is
    // `PDO_SBX_SESSION=pdo-<run>-<node>-…`, a distinct prefix), so the match is
    // unambiguous.
    let script = format!(
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$@\" >> {log}\n\
         case \"$1\" in\n\
         image) exit 0 ;;\n\
         container) printf 'true'; exit 0 ;;\n\
         exec)\n\
         shift\n\
         while [ \"$#\" -gt 0 ]; do\n\
         case \"$1\" in\n\
         pdo-sbx-*) shift; break ;;\n\
         *) shift ;;\n\
         esac\n\
         done\n\
         exec \"$@\"\n\
         ;;\n\
         *) exit 0 ;;\n\
         esac\n",
        log = sq(&log.display().to_string()),
    );
    std::fs::write(&bin, &script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, bin.to_str().unwrap().to_string(), log)
}

fn log_text(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

async fn start_run(daemon: &TestDaemon, sandbox: Option<&str>) -> String {
    let mut body = serde_json::json!({ "pipeline": "sbx-obs", "input": "hello" });
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

async fn post_command(
    daemon: &TestDaemon,
    run_id: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// Write the worker's declared `out` artifact so node-done's output validation
/// passes (host path == container path via the identity mount).
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
        .post(format!(
            "{}/runs/{run_id}/nodes/{NODE_ID}/done",
            daemon.url()
        ))
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

/// The Run's pipeline worktree (the doc-only worker's cwd — non-CM nodes run
/// there). Both the cost prefix and the stale probe encode THIS path.
fn worktree_dir(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
}

/// The staged Claude `projects/` root for a sandboxed run (under the tempdir home).
fn staging_projects(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/sandbox")
        .join(run_id)
        .join("claude-home/projects")
}

/// The host `~/.claude/projects/` root (the tempdir home override).
fn host_projects(daemon: &TestDaemon) -> PathBuf {
    daemon.repo_root().join(".claude/projects")
}

/// One priced assistant transcript line: `input` opus-4-8 tokens (× $5/MTok).
fn priced_line(id: &str, req: &str, input: u64) -> String {
    format!(
        r#"{{"type":"assistant","requestId":"{req}","message":{{"id":"{id}","model":"claude-opus-4-8","usage":{{"input_tokens":{input},"output_tokens":0}}}}}}"#
    )
}

/// Plant a transcript for `run_id`'s worktree cwd under `projects_root`, returning
/// the `.jsonl` path. Optionally back-date it so the stale sweep sees it idle.
fn plant_transcript(
    projects_root: &Path,
    daemon: &TestDaemon,
    run_id: &str,
    body: &str,
    age: Option<Duration>,
) -> PathBuf {
    let enc = encode_working_dir(&worktree_dir(daemon, run_id));
    let proj = projects_root.join(enc);
    std::fs::create_dir_all(&proj).unwrap();
    let file = proj.join("s.jsonl");
    std::fs::write(&file, body).unwrap();
    if let Some(age) = age {
        filetime::set_file_mtime(
            &file,
            filetime::FileTime::from_system_time(SystemTime::now() - age),
        )
        .unwrap();
    }
    file
}

// -- Test 1: cost reads the staged home while a sandboxed run is live ----------

#[tokio::test]
async fn cost_reads_staging_during_minimal_run() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_live_docker();
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        docker,
        Some("exec sleep 600".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    // The worker reaches Running with a live (sleeping) session; its staging is
    // seeded by the eager prep.
    wait_node_status(&daemon, &run_id, "running").await;
    assert!(
        wait_until(|| staging_projects(&daemon, &run_id)
            .parent()
            .unwrap()
            .exists())
        .await,
        "the staging home must exist during a live sandboxed run"
    );

    // Staging: $5 (1M opus input). Host: $10 (2M) — bigger, to prove the seam
    // sources the STAGING while the run is live, not `~/.claude`.
    plant_transcript(
        &staging_projects(&daemon, &run_id),
        &daemon,
        &run_id,
        &format!("{}\n", priced_line("m1", "r1", 1_000_000)),
        None,
    );
    plant_transcript(
        &host_projects(&daemon),
        &daemon,
        &run_id,
        &format!("{}\n", priced_line("m2", "r2", 2_000_000)),
        None,
    );

    let run = get_run(&daemon, &run_id).await;
    let usd = run["cost"]["usd"].as_f64().unwrap_or(-1.0);
    assert!(
        (usd - 5.0).abs() < 1e-9,
        "cost must read the STAGED transcript ($5) during a live sandboxed run, \
         not the larger host transcript ($10): cost = {}",
        run["cost"]
    );
}

// -- Test 2: stale-detection reads the staged home while live ------------------

#[tokio::test]
async fn stale_detection_reads_staging_during_minimal_run() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_live_docker();
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        docker,
        Some("exec sleep 600".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    assert!(
        wait_until(|| staging_projects(&daemon, &run_id)
            .parent()
            .unwrap()
            .exists())
        .await,
        "the staging home must exist during a live sandboxed run"
    );

    // Idle (back-dated 300s) transcript in the STAGING only; outputs incomplete
    // (worker `out` never written). If the sweep read `~/.claude` (empty) the
    // node would stay Running — so `stale` proves it read the staging.
    plant_transcript(
        &staging_projects(&daemon, &run_id),
        &daemon,
        &run_id,
        "{}\n",
        Some(Duration::from_secs(300)),
    );

    daemon.run_stale_detection_tick().await;

    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["nodes"][NODE_ID]["status"], "stale",
        "stale-detection must read the STAGED transcript for a live sandboxed run: {run}"
    );
}

// -- Test 3: terminal transition merges staging into ~/.claude/projects --------

#[tokio::test]
async fn terminal_merges_staging_into_host_claude_projects() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_live_docker();
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        docker,
        Some("exec sleep 600".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    assert!(
        wait_until(|| staging_projects(&daemon, &run_id)
            .parent()
            .unwrap()
            .exists())
        .await,
        "staging must exist before the terminal merge"
    );
    let body = format!("{}\n", priced_line("m1", "r1", 1_000_000));
    plant_transcript(
        &staging_projects(&daemon, &run_id),
        &daemon,
        &run_id,
        &body,
        None,
    );

    // Drive the run terminal (worker output present → run completes).
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    // The detached terminal merge lands the transcript in the host projects dir
    // at the standard encoded dirname, verbatim.
    let enc = encode_working_dir(&worktree_dir(&daemon, &run_id));
    let host_file = host_projects(&daemon).join(&enc).join("s.jsonl");
    assert!(
        wait_until(|| host_file.is_file()).await,
        "terminal merge must copy the transcript to {host_file:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&host_file).unwrap(),
        body,
        "the host transcript must be a verbatim copy of the staged one"
    );
    // Cost is still visible post-terminal (staging present → seam reads staging;
    // either way the value is $5).
    let run = get_run(&daemon, &run_id).await;
    assert!(
        (run["cost"]["usd"].as_f64().unwrap_or(-1.0) - 5.0).abs() < 1e-9,
        "cost must stay $5 after the terminal transition: {run}"
    );
}

// -- Test 4: double merge (terminal then cleanup) is byte-identical ------------

#[tokio::test]
async fn double_merge_terminal_then_cleanup_is_identical() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    ensure_pdo_on_path();
    let (_fake, docker, _log) = write_live_docker();
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        docker,
        Some("exec sleep 600".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    assert!(
        wait_until(|| staging_projects(&daemon, &run_id)
            .parent()
            .unwrap()
            .exists())
        .await,
        "staging must exist"
    );
    let body = format!("{}\n", priced_line("m1", "r1", 1_000_000));
    plant_transcript(
        &staging_projects(&daemon, &run_id),
        &daemon,
        &run_id,
        &body,
        None,
    );

    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    let enc = encode_working_dir(&worktree_dir(&daemon, &run_id));
    let host_dir = host_projects(&daemon).join(&enc);
    let host_file = host_dir.join("s.jsonl");
    assert!(
        wait_until(|| host_file.is_file()).await,
        "terminal merge must produce {host_file:?}"
    );
    let after_terminal = std::fs::read(&host_file).unwrap();

    // Second merge at cleanup_run (before teardown), then archive.
    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "cleanup_run" }),
    )
    .await;
    assert!(resp.status().is_success(), "cleanup_run should archive");
    wait_run_status(&daemon, &run_id, "archived").await;

    // Byte-identical, exactly one file (no duplicate / `*.tmp` residue), and the
    // staging is gone.
    assert_eq!(
        std::fs::read(&host_file).unwrap(),
        after_terminal,
        "the cleanup merge must be byte-identical to the terminal merge (idempotent)"
    );
    let files: Vec<_> = std::fs::read_dir(&host_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files,
        vec!["s.jsonl".to_string()],
        "exactly one merged file, no duplicate/tmp residue: {files:?}"
    );
    assert!(
        !daemon
            .repo_root()
            .join(".pdo/sandbox")
            .join(&run_id)
            .exists(),
        "cleanup must purge the staging after the merge"
    );
}

// -- Test 5: resume re-arms the container (ensure_ready) -----------------------

#[tokio::test]
async fn resume_reengages_seam_and_ensures_container() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    ensure_pdo_on_path();
    let (_fake, docker, log) = write_live_docker();
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        docker,
        Some("exec sleep 600".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_node_status(&daemon, &run_id, "running").await;
    write_node_output(&daemon, &run_id, "done\n");
    simulate_node_done(&daemon, &run_id).await;
    wait_run_status(&daemon, &run_id, "completed").await;

    // Count container probes so far; resuming must run ensure_ready again (which
    // always probes the container), so the count strictly increases.
    let probes_before = log_text(&log).lines().filter(|l| *l == "container").count();

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "resume_run" }),
    )
    .await;
    assert_eq!(
        resp.status(),
        200,
        "resume of a sandboxed run must return OK once the container is ensured"
    );

    assert!(
        wait_until(|| log_text(&log).lines().filter(|l| *l == "container").count() > probes_before)
            .await,
        "resume must re-arm the container via ensure_ready (a fresh container probe); log:\n{}",
        log_text(&log)
    );
}

// -- Test 6: resume fails loud when Docker is unavailable ----------------------

#[tokio::test]
async fn resume_fails_loud_when_docker_unavailable() {
    ensure_pdo_on_path();
    // A docker binary that does not exist: eager prep fails → RunFailed, and the
    // resume guard's ensure_ready fails too → explicit 500, never a host fallback.
    let daemon = TestDaemon::spawn_with_docker_and_tmux_override(
        seed,
        "/nonexistent/pdo-fake-docker-obs".to_string(),
        Some("exec true".to_string()),
    )
    .await
    .unwrap();

    let run_id = start_run(&daemon, Some("minimal")).await;
    wait_run_status(&daemon, &run_id, "failed").await;

    let resp = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "resume_run" }),
    )
    .await;
    assert_eq!(
        resp.status(),
        500,
        "resuming a sandboxed run with Docker unavailable must fail loud, not fall back to the host"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("sandbox container unavailable"),
        "the 500 must name the sandbox-container failure: {body}"
    );
}
