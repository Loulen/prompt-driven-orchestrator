//! Layer 3a — #469: liveness detection through the *real* daemon sweep.
//!
//! Replaces `stale_mtime_detection.rs` (#373), whose subject — the mtime-based
//! `Stale` / `AutoComplete` verdicts — no longer exists. What is proven here:
//!
//! - a node silent past the old 120 s threshold stays `Running`, and neither
//!   `node_stale` nor `run_failed` is emitted (AC1 / AC2 — the bug);
//! - with the setting **off**, a node that visibly finished its turn with valid
//!   outputs is left alone (AC8);
//! - with the setting **on**, that node is completed through the *same* body as
//!   `POST …/done`, and a `code-mutating` node's commit lands on the pipeline
//!   branch (AC9 — the §3 defect);
//! - flipping the setting takes effect on the next sweep, no restart (AC10).
//!
//! `sandbox_home_override` is the seam that makes a planted transcript readable
//! (see `TestDaemon::spawn_with_home_override`), so no test here mutates the
//! process-global `HOME` and the binary can hold several of them.
//!
//! **Honest limitation.** These nodes run the harness tmux tail (`sleep`), not a
//! real `claude`, so the transcript is planted rather than written. The
//! *transcript → verdict* half is layer 1 on fixtures cut from the real
//! transcript (`stale_detector::tests`); the *verdict → Run outcome* half is
//! here. A genuine agent node with a long silent tool call is the manual layer-5
//! recipe in the issue — `PDO_TMUX_CMD_OVERRIDE` suppresses the transcript
//! entirely, so the usual test seam structurally cannot reproduce it.

mod common;

use std::time::{Duration, SystemTime};

use common::TestDaemon;
use pdo_daemon::stale_detector;
use pdo_daemon::tmux_session_manager;

const DOC_PIPELINE: &str = "turn-end-doc";
const CM_PIPELINE: &str = "turn-end-cm";
const NODE_ID: &str = "worker";

const DOC_PIPELINE_YAML: &str = r#"name: turn-end-doc
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

const CM_PIPELINE_YAML: &str = r#"name: turn-end-cm
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: code-mutating
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

/// The real tail of the transcript this issue was opened about: an assistant
/// `text` message, then `system` records, then untimestamped metadata. Parses as
/// `TurnEnded`.
const FIXTURE_TURN_ENDED: &str = include_str!("fixtures/turn_state/turn_ended.jsonl");
/// The same transcript cut on a `docker build -q` whose `tool_result` landed
/// 214 s later. Parses as `InToolCall` — alive, whatever the silence.
const FIXTURE_IN_TOOL_CALL: &str = include_str!("fixtures/turn_state/in_tool_call.jsonl");

// --- harness -----------------------------------------------------------------

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

fn git(repo: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
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
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Seed the repo with both pipelines and an initial commit.
fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{DOC_PIPELINE}.yaml")),
        DOC_PIPELINE_YAML,
    )?;
    std::fs::write(
        pipelines_dir.join(format!("{CM_PIPELINE}.yaml")),
        CM_PIPELINE_YAML,
    )?;

    git(repo, &["init", "-q", "-b", "main"])?;
    git(repo, &["config", "user.email", "test@example.com"])?;
    git(repo, &["config", "user.name", "Test"])?;
    git(repo, &["config", "commit.gpgsign", "false"])?;
    // Keep git's global config reachable (worktree creation) without inheriting
    // the tempdir HOME the daemon uses for transcripts.
    if let Some(real) = std::env::var_os("HOME") {
        git(
            repo,
            &[
                "config",
                "include.path",
                &std::path::Path::new(&real)
                    .join(".gitconfig")
                    .to_string_lossy(),
            ],
        )
        .ok();
    }
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    git(repo, &["add", "."])?;
    git(repo, &["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon: &TestDaemon, pipeline: &str) -> String {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs must create the run");
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

async fn run_status(daemon_url: &str, run_id: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["status"].as_str().map(String::from)
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

/// Set (or clear) the turn-end auto-completion knob through the real
/// `PUT /settings`, and assert the recomputed view agrees.
async fn set_autocomplete(daemon_url: &str, on: bool) {
    let resp = reqwest::Client::new()
        .put(format!("{daemon_url}/settings"))
        .json(&serde_json::json!({ "autocomplete_turn_end": on }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PUT /settings must accept the flag");
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        view["autocomplete_turn_end"]["effective"],
        serde_json::json!(on),
        "the recomputed view must disclose the new effective value"
    );
    assert_eq!(
        view["autocomplete_turn_end"]["source"],
        serde_json::json!("stored"),
        "an explicit save is a STORED decision, in both directions"
    );
}

/// Plant a Claude Code transcript for `working_dir` under the daemon's (temp)
/// home, back-dated `quiet` so the sweep sees the write as settled.
///
/// `session_id` is the id PDO pinned for the node at spawn
/// ([`TestDaemon::pinned_session_id`]), and it names the file. That is not a
/// cosmetic detail: since #473 the sweep resolves a node's transcript by exact
/// filename (`<session_id>.jsonl`) rather than by picking the newest `.jsonl` in
/// the encoded-cwd dir, so a fixture planted under any other name resolves to
/// nothing and every assertion here degrades into "the sweep had no signal" —
/// which is indistinguishable from "the sweep read it and decided to do nothing".
/// Naming it as production would is what keeps the no-op assertions falsifiable.
fn plant_transcript(
    home: &std::path::Path,
    working_dir: &std::path::Path,
    session_id: &str,
    contents: &str,
    quiet: Duration,
) {
    let encoded = stale_detector::encode_working_dir(working_dir);
    let proj = home.join(".claude").join("projects").join(encoded);
    std::fs::create_dir_all(&proj).unwrap();
    let jsonl = proj.join(format!("{session_id}.jsonl"));
    std::fs::write(&jsonl, contents).unwrap();
    filetime::set_file_mtime(
        &jsonl,
        filetime::FileTime::from_system_time(SystemTime::now() - quiet),
    )
    .unwrap();
}

/// Write the `worker` node's declared `out` artifact so outputs validation passes.
fn write_valid_outputs(worktree: &std::path::Path) {
    let out_dir = worktree.join(".pdo/artifacts/worker/iter-1/out");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("output.md"), "# Output\nDone.").unwrap();
}

/// The `quiet` age a settled transcript needs to clear the anti-bounce window.
fn settled() -> Duration {
    stale_detector::TURN_END_QUIET_PERIOD + Duration::from_secs(30)
}

fn kill_session(socket: &str, session: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["-L", socket, "kill-session", "-t", session])
        .output();
}

// --- AC1 / AC2: silence is not death ----------------------------------------

/// The bug. A node silent far past the old 120 s threshold — because it is inside
/// a `docker build` — must stay `Running`, and the Run must stay `Running`. No
/// `node_stale`, no `run_failed`.
///
/// The setting is deliberately **on** here: a pending `tool_use` must be immune
/// even when the feature that could complete a node is armed, and even with its
/// outputs already valid (the "two writers on one worktree" hazard).
#[tokio::test]
async fn a_long_silent_tool_call_never_kills_the_run() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    set_autocomplete(&daemon.url(), true).await;

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(
        wait_for_session(&socket, &session, Duration::from_secs(5)).await,
        "the node session should appear"
    );

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    // Outputs ALREADY valid, so only the turn-state guard stands between this
    // node and a completion it must not get.
    write_valid_outputs(&worktree);
    let sid = daemon.pinned_session_id(&run_id, NODE_ID).await;
    plant_transcript(
        &home,
        &worktree,
        &sid,
        FIXTURE_IN_TOOL_CALL,
        Duration::from_secs(300),
    );

    daemon.run_stale_detection_tick().await;

    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("running"),
        "a node inside a long tool call must stay Running (#469)"
    );
    assert_eq!(
        run_status(&daemon.url(), &run_id).await.as_deref(),
        Some("running"),
        "and the Run must stay Running"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        !kinds.iter().any(|k| k == "node_stale"),
        "no node_stale may be emitted any more; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "run_failed"),
        "no run_failed may follow; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "node_auto_completed"),
        "a pending tool call must not be auto-completed; saw {kinds:?}"
    );

    kill_session(&socket, &session);
}

/// The old `Stale` trigger, exactly: a transcript idle past the threshold with
/// **incomplete** outputs. It used to produce `node_stale` and a `run_failed`
/// 27 ms later. It must now produce nothing at all.
#[tokio::test]
async fn an_idle_transcript_with_incomplete_outputs_is_no_longer_stale() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    // No outputs written: incomplete, the old `Detection::Stale` arm.
    let sid = daemon.pinned_session_id(&run_id, NODE_ID).await;
    plant_transcript(&home, &worktree, &sid, "{}\n", Duration::from_secs(600));

    daemon.run_stale_detection_tick().await;

    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("running")
    );
    assert_eq!(
        run_status(&daemon.url(), &run_id).await.as_deref(),
        Some("running"),
        "the run-level stall reconciler must not see a dead node (#469 §1 cascade)"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        !kinds.iter().any(|k| k == "node_stale" || k == "run_failed"),
        "saw {kinds:?}"
    );

    kill_session(&socket, &session);
}

/// Session death is still the authority: kill the session out of band and the
/// node must fail with a cause naming it.
#[tokio::test]
async fn session_death_is_still_detected() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);
    kill_session(&socket, &session);

    daemon.run_stale_detection_tick().await;

    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("interrupted"),
        "a dead session is still the one verdict of death — but ADR-0049 makes it \
         `Interrupted` (recoverable), not `Failed`"
    );
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/events", daemon.url()))
        .send()
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = resp.json().await.unwrap();
    let reason = events
        .iter()
        .find(|e| e["kind"] == "node_interrupted")
        .and_then(|e| e["payload"]["reason"].as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        reason.contains("session_died") && reason.contains(&session),
        "the cause must name the dead session; got {reason:?}"
    );
}

// --- AC8 / AC10: the setting ------------------------------------------------

/// AC8 + AC10 in one Run, because they are two halves of the same statement: the
/// box decides, and it decides *live*.
///
/// A node that has visibly finished its turn with valid outputs is left strictly
/// alone while the box is unchecked; ticking it completes the node on the very
/// next sweep, with no daemon restart.
#[tokio::test]
async fn the_setting_gates_completion_and_takes_effect_on_the_next_sweep() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    // Default: unset ⇒ off (ADR-0012). Assert the disclosed view says so before
    // touching anything.
    let view: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/settings", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        view["autocomplete_turn_end"]["effective"],
        serde_json::json!(false),
        "turn-end auto-completion must be OFF on a fresh instance"
    );
    assert_eq!(
        view["autocomplete_turn_end"]["default"],
        serde_json::json!(false)
    );

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    write_valid_outputs(&worktree);
    let sid = daemon.pinned_session_id(&run_id, NODE_ID).await;
    plant_transcript(&home, &worktree, &sid, FIXTURE_TURN_ENDED, settled());

    // --- box unchecked: nothing happens (AC8) ---
    daemon.run_stale_detection_tick().await;
    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("running"),
        "with the setting off, a finished turn must be left alone"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        !kinds
            .iter()
            .any(|k| k == "node_auto_completed" || k == "node_completed"),
        "saw {kinds:?}"
    );

    // --- tick the box; NO restart (AC10) ---
    set_autocomplete(&daemon.url(), true).await;
    daemon.run_stale_detection_tick().await;

    // The completion tail is detached (#304 / ADR-0023), so poll.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref()
            == Some("completed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("completed"),
        "ticking the box must take effect on the very next sweep, with no restart"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        kinds.iter().any(|k| k == "node_auto_completed"),
        "the log must say the completion was AUTOMATIC, not a plain node_completed; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "node_completed"),
        "and not both; saw {kinds:?}"
    );
}

// --- #433 / ADR-0043: the Stop hook's `pdo complete --auto` wire path ---------

/// POST `…/done` with `{ "iter": …, "auto": … }`, the exact body
/// `pdo complete --auto` sends. Returns the HTTP status.
async fn post_done(daemon_url: &str, run_id: &str, node: &str, iter: i64, auto: bool) -> u16 {
    reqwest::Client::new()
        .post(format!("{daemon_url}/runs/{run_id}/nodes/{node}/done"))
        .json(&serde_json::json!({ "iter": iter, "auto": auto }))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// The Stop hook's completion goes through the SAME `…/done` body as the agent,
/// but with `auto: true` — so it records `NodeAutoCompleted` (the log says
/// "automatic"), never a plain `NodeCompleted`. This pins the source wiring
/// (`auto → CompletionSource::StopHook`) end to end through the HTTP handler.
#[tokio::test]
async fn done_with_auto_true_records_node_auto_completed() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    write_valid_outputs(&worktree);

    let status = post_done(&daemon.url(), &run_id, NODE_ID, 1, true).await;
    assert_eq!(status, 200, "a valid auto-completion is granted");

    // The completion tail is detached (#304 / ADR-0023), so poll.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref()
            == Some("completed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        kinds.iter().any(|k| k == "node_auto_completed"),
        "auto:true must record node_auto_completed; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "node_completed"),
        "auto:true must NOT record a plain node_completed; saw {kinds:?}"
    );

    kill_session(&socket, &session);
}

/// The other side of the wire: `auto: false` (or a legacy body without the field)
/// stays the agent-typed `Explicit` path → `NodeCompleted`. Proven together so a
/// regression that hard-wires one source can't hide behind the other.
#[tokio::test]
async fn done_with_auto_false_records_plain_node_completed() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    write_valid_outputs(&worktree);

    let status = post_done(&daemon.url(), &run_id, NODE_ID, 1, false).await;
    assert_eq!(status, 200);

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref()
            == Some("completed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        kinds.iter().any(|k| k == "node_completed"),
        "auto:false is the agent-typed path → node_completed; saw {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "node_auto_completed"),
        "auto:false must NOT record node_auto_completed; saw {kinds:?}"
    );

    kill_session(&socket, &session);
}

/// Idempotence hook ↔ fallback (the property that makes `pdo complete --auto`
/// safe to run on *every* turn end): once a node is complete, a second `…/done`
/// with `auto: true` — the hook firing after the sweep already won, say — records
/// **no second terminal event** and leaves the node completed. The daemon's exact
/// status here is topology-dependent (a legal-duplicate `NoOp` 200 while the run
/// is still live — proven source-agnostically in `cli_complete_does_not_panic` —
/// or the terminal-run refusal once the run has advanced to `Completed`); either
/// way the hook's `; exit 0` neutralises it and, crucially, the completion guard
/// never doubles the terminal event.
#[tokio::test]
async fn a_repeat_auto_completion_never_doubles_the_terminal_event() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    write_valid_outputs(&worktree);

    assert_eq!(
        post_done(&daemon.url(), &run_id, NODE_ID, 1, true).await,
        200
    );
    // Let the detached completion settle.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref()
            == Some("completed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Second auto-completion. Never a 5xx breakdown (that would be the daemon
    // failing to decide), and — the load-bearing assertion — no double completion.
    let second = post_done(&daemon.url(), &run_id, NODE_ID, 1, true).await;
    assert!(
        second < 500,
        "a repeat auto-completion must get a verdict, not a 5xx breakdown; got {second}"
    );
    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("completed"),
        "the node stays completed after a repeat auto-completion"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    let terminal = kinds
        .iter()
        .filter(|k| *k == "node_auto_completed" || *k == "node_completed")
        .count();
    assert_eq!(terminal, 1, "no double completion; saw {kinds:?}");

    kill_session(&socket, &session);
}

/// Safety of the `; exit 0` wrapper: an auto-completion with the output still
/// missing is a **recoverable** refusal (409), and — critically — nothing
/// terminal is recorded (ADR-0035). So the hook's `exit 3` (swallowed by
/// `; exit 0`) leaves the node running for the fallback / a human, never wedged.
#[tokio::test]
async fn auto_done_with_missing_output_is_recoverable_and_records_nothing() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();

    let run_id = create_run(&daemon, DOC_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    // No outputs written → the completion is refused, recoverably.
    let status = post_done(&daemon.url(), &run_id, NODE_ID, 1, true).await;
    assert_eq!(status, 409, "missing output ⇒ recoverable refusal, not 2xx");

    // Give any (erroneous) terminal append a beat to land, then assert none did.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("running"),
        "a refused auto-completion must leave the node running"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        !kinds
            .iter()
            .any(|k| k == "node_auto_completed" || k == "node_completed" || k == "node_failed"),
        "a recoverable refusal records nothing terminal (ADR-0035); saw {kinds:?}"
    );

    kill_session(&socket, &session);
}

// --- AC9: the §3 defect — the commit must reach the pipeline branch ----------

/// The test that catches the defect of the old design: on a `code-mutating` node,
/// auto-completion must go through the same body as `POST …/done` —
/// `commit_and_merge_sub_worktree_inner` included — or the node records
/// `Completed` with its work stranded on `pdo/sub-…` and the downstream gets
/// nothing.
#[tokio::test]
async fn auto_completing_a_code_mutating_node_merges_its_commit() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let home = daemon.repo_root().to_path_buf();

    set_autocomplete(&daemon.url(), true).await;

    let run_id = create_run(&daemon, CM_PIPELINE).await;
    let session = tmux_session_manager::node_session_name(&run_id, NODE_ID, 1);
    assert!(wait_for_session(&socket, &session, Duration::from_secs(5)).await);

    let worktree = home.join(".pdo/runs").join(&run_id).join("worktree");
    // A code-mutating node works in its OWN sub-worktree, and that is the cwd its
    // transcript is keyed on.
    let sub_worktree = home
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");
    assert!(
        sub_worktree.exists(),
        "the sub-worktree must exist after spawn: {}",
        sub_worktree.display()
    );

    // The work the agent "did", uncommitted — exactly what an agent that finished
    // and forgot to call `pdo complete` leaves behind.
    std::fs::write(sub_worktree.join("IMPLEMENTED.md"), "the work\n").unwrap();
    write_valid_outputs(&worktree);
    let sid = daemon.pinned_session_id(&run_id, NODE_ID).await;
    plant_transcript(&home, &sub_worktree, &sid, FIXTURE_TURN_ENDED, settled());

    daemon.run_stale_detection_tick().await;

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref()
            == Some("completed")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(
        node_status(&daemon.url(), &run_id, NODE_ID)
            .await
            .as_deref(),
        Some("completed"),
        "the finished code-mutating node must auto-complete"
    );
    let kinds = event_kinds(&daemon.url(), &run_id).await;
    assert!(
        kinds.iter().any(|k| k == "node_auto_completed"),
        "saw {kinds:?}"
    );

    // The point of the test: the work is ON THE PIPELINE BRANCH, not stranded on
    // `pdo/sub-…`. A bare `NodeAutoCompleted` append would pass every assertion
    // above and fail this one.
    assert!(
        worktree.join("IMPLEMENTED.md").exists(),
        "the sub-worktree commit must be merged into the pipeline worktree — a \
         completion that skips commit_and_merge_sub_worktree_inner strands it"
    );
    let log = git(&worktree, &["log", "--oneline", "-5"]).unwrap();
    assert!(
        log.contains(NODE_ID),
        "the pipeline branch must carry the node's commit; log:\n{log}"
    );

    kill_session(&socket, &session);
}
