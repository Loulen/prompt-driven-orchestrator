//! Layer 3a — #373: the mtime-based `Stale` / `AutoComplete` branches, dead in
//! production for so long because `encode_working_dir` resolved the wrong
//! Claude Code project dir, are proven end-to-end through the *real* daemon
//! sweep here (ADR-0004 règle d'or: a resilience invariant is closed at layer
//! ≥3, not with fake-probe unit tests alone).
//!
//! The sweep reads `$HOME/.claude/projects/<encode_working_dir(dir)>` for a
//! node's transcript. This binary sets `HOME` process-global to a temp dir and
//! plants a back-dated transcript at the exact encoded path — the whole point of
//! the #373 encoder fix is that this path now resolves. A single test keeps the
//! process-global `HOME` uncontended.

mod common;

use std::time::{Duration, SystemTime};

use common::TestDaemon;
use pdo_daemon::stale_detector;
use pdo_daemon::tmux_session_manager;

const PIPELINE_NAME: &str = "stale-mtime";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: stale-mtime
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
"#;

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

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
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

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "test input" });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn wait_for_session(socket: &str, session: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tmux_has_session(socket, session) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn node_status(daemon_url: &str, run_id: &str, node: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["nodes"][node]["status"].as_str().map(String::from)
}

async fn event_kinds(daemon_url: &str, run_id: &str) -> Vec<String> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = resp.json().await.unwrap();
    events
        .iter()
        .filter_map(|e| e["kind"].as_str().map(String::from))
        .collect()
}

/// Plant a Claude Code transcript for `working_dir` under the (temp) HOME, its
/// mtime back-dated `age` so the sweep sees the node idle past the threshold.
fn plant_idle_transcript(home: &std::path::Path, working_dir: &std::path::Path, age: Duration) {
    let encoded = stale_detector::encode_working_dir(working_dir);
    let proj = home.join(".claude").join("projects").join(encoded);
    std::fs::create_dir_all(&proj).unwrap();
    let jsonl = proj.join("session.jsonl");
    std::fs::write(&jsonl, "{}\n").unwrap();
    filetime::set_file_mtime(
        &jsonl,
        filetime::FileTime::from_system_time(SystemTime::now() - age),
    )
    .unwrap();
}

/// #373 end-to-end: with the encoder fixed, an idle node's transcript resolves,
/// so `decide` reaches the `Stale` / `AutoComplete` arms that were dead in prod.
/// One sweep must
///   (a) mark an idle node with **incomplete** outputs `stale` (re-armed live),
///   (b) leave an idle node with **valid** outputs `running` and emit the
///       non-terminal `node_auto_complete_observed` marker (observe-only, Unit
///       A) — never the terminal `node_auto_completed`.
#[tokio::test]
async fn idle_transcript_reactivates_stale_and_observe_only_autocomplete() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // The sweep reads $HOME; point it at a temp dir we own so planting a
    // transcript can't touch the real ~/.claude. Single test in this binary, so
    // the process-global set is uncontended. Preserve real HOME for git's global
    // config (worktree creation) via GIT_CONFIG_GLOBAL.
    let home = tempfile::tempdir().unwrap();
    if let Some(real) = std::env::var_os("HOME") {
        std::env::set_var(
            "GIT_CONFIG_GLOBAL",
            std::path::Path::new(&real).join(".gitconfig"),
        );
    }
    std::env::set_var("HOME", home.path());

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    let repo_root = daemon.repo_root().to_path_buf();

    // --- Run A: idle + incomplete outputs → Stale ---
    let run_stale = create_run(&daemon.url()).await;
    let session_a = tmux_session_manager::node_session_name(&run_stale, NODE_ID, 1);
    assert!(
        wait_for_session(&socket, &session_a, Duration::from_secs(5)).await,
        "run A node session should appear"
    );
    let workdir_a = repo_root
        .join(".pdo/runs")
        .join(&run_stale)
        .join("worktree");
    plant_idle_transcript(home.path(), &workdir_a, Duration::from_secs(300));

    // --- Run B: idle + VALID outputs → AutoComplete (observe-only) ---
    let run_observe = create_run(&daemon.url()).await;
    let session_b = tmux_session_manager::node_session_name(&run_observe, NODE_ID, 1);
    assert!(
        wait_for_session(&socket, &session_b, Duration::from_secs(5)).await,
        "run B node session should appear"
    );
    let workdir_b = repo_root
        .join(".pdo/runs")
        .join(&run_observe)
        .join("worktree");
    // Write the worker's declared `out` artifact so validation passes.
    let out_dir = workdir_b.join(".pdo/artifacts/worker/iter-1/out");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("output.md"), "# Output\nDone.").unwrap();
    plant_idle_transcript(home.path(), &workdir_b, Duration::from_secs(300));

    // One real sweep handles both runs.
    daemon.run_stale_detection_tick().await;

    // (a) Re-armed Stale: the node is marked stale (session stays alive; this is
    // the intended #251 safety net, recoverable via resume_run).
    assert_eq!(
        node_status(&daemon.url(), &run_stale, NODE_ID)
            .await
            .as_deref(),
        Some("stale"),
        "#373: an idle node with incomplete outputs must be marked stale — the \
         branch that was dead in prod before the encoder fix"
    );

    // (b) Observe-only AutoComplete: node stays running, marker emitted, and the
    // terminal event is NOT appended.
    assert_eq!(
        node_status(&daemon.url(), &run_observe, NODE_ID)
            .await
            .as_deref(),
        Some("running"),
        "#373 Unit A: auto-complete is observe-only — the node must stay Running"
    );
    let kinds = event_kinds(&daemon.url(), &run_observe).await;
    assert!(
        kinds.iter().any(|k| k == "node_auto_complete_observed"),
        "observe-only path must emit node_auto_complete_observed; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "node_auto_completed"),
        "observe-only path must NOT emit the terminal node_auto_completed; saw {kinds:?}"
    );

    // Cleanup best-effort.
    for s in [&session_a, &session_b] {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &socket, "kill-session", "-t", s])
            .output();
    }
}
