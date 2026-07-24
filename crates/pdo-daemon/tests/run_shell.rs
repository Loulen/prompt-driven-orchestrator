//! Layer 3a — "Open session": ad-hoc bash shell in a terminal run's pipeline
//! worktree (#316 / ADR-0021).
//!
//! Drives `POST /sessions/{run_id}/shell` against a real daemon and corroborates
//! every side effect out-of-band on the daemon's private tmux socket + the run's
//! worktree on disk. The shell is a real `bash -i` (it IS deterministic bash, so
//! it bypasses the `tmux_cmd_override` test seam entirely — the daemon's default
//! `exec sleep 600` override does not touch it).
//!
//! To obtain a *terminal, non-archived, worktree-present* run reliably and fast,
//! we drive a `start → script → end` pipeline whose script body `exit 1`s: the
//! run reaches `failed` quickly with its pipeline worktree intact (a
//! `start→…→end` completed run flips lazily ~30 s — a failed run is prompt). The
//! script wrapper calls bare `pdo fail`, so the built `pdo` must be on PATH.

mod common;

use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use common::TestDaemon;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

const FAIL_PIPELINE: &str = "shell-fail";
const LIVE_PIPELINE: &str = "shell-live";
const SCRIPT_NODE: &str = "notify";

/// `start → script → end`. The script body is seeded per-test into the node's
/// prompt slot; here it always `exit 1`s so the run reaches `failed`.
const FAIL_YAML: &str = r#"name: shell-fail
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

/// `start → worker → end` with a `code-mutating` worker. Under the daemon's
/// default `exec sleep 600` override the worker never completes, so the run
/// stays `running` (live) — the negative case for the eligibility gate.
const LIVE_YAML: &str = r#"name: shell-live
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

/// Seed a script pipeline + its bash body.
fn seed_script(
    yaml: &'static str,
    name: &'static str,
    body: &'static str,
) -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
    move |repo: &std::path::Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml)?;
        let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(prompts_dir.join(format!("{SCRIPT_NODE}.md")), body)?;
        git_init_with_commit(repo)?;
        Ok(())
    }
}

/// Seed a plain pipeline (no per-node prompt body).
fn seed_plain(
    yaml: &'static str,
    name: &'static str,
) -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
    move |repo: &std::path::Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml)?;
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

/// Poll `GET /runs` until the run reaches `expected` status, or time out.
///
/// The deadline is generous (60 s) on purpose: this suite spawns many real
/// daemons — each with its own tmux server and `pdo` child processes — and runs
/// them in parallel, so a run can legitimately take tens of seconds to reach a
/// terminal status under that contention. A tight deadline flakes even though the
/// run *did* transition (the failure message then paradoxically prints the
/// expected status).
async fn wait_for_run_status(daemon: &TestDaemon, run_id: &str, expected: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        if run_status(daemon, run_id).await.as_deref() == Some(expected) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

async fn run_status(daemon: &TestDaemon, run_id: &str) -> Option<String> {
    let runs: serde_json::Value = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    runs.as_array()?
        .iter()
        .find(|r| r["run_id"].as_str() == Some(run_id))
        .and_then(|r| r["status"].as_str())
        .map(String::from)
}

async fn post_shell(daemon: &TestDaemon, run_id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/sessions/{run_id}/shell", daemon.url()))
        .send()
        .await
        .unwrap()
}

/// Number of `pdo-shell-*` sessions on the daemon's socket.
fn shell_session_count(socket: &str) -> usize {
    pdo_daemon::tmux_session_manager::list_pdo_sessions(socket)
        .into_iter()
        .filter(|s| s.starts_with("pdo-shell-"))
        .count()
}

fn worktree_path(daemon: &TestDaemon, run_id: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

/// Drive a run to `failed` (script `exit 1`) and return its id, asserting the
/// pipeline worktree is present (the shell-eligibility precondition).
async fn failed_run(daemon: &TestDaemon) -> String {
    let run_id = start_run(daemon, FAIL_PIPELINE).await;
    assert!(
        wait_for_run_status(daemon, &run_id, "failed").await,
        "run must reach failed; status was {:?}",
        run_status(daemon, &run_id).await
    );
    assert!(
        worktree_path(daemon, &run_id).exists(),
        "a failed run keeps its pipeline worktree on disk"
    );
    run_id
}

#[tokio::test]
async fn open_shell_creates_session_on_failed_run() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;

    let resp = post_shell(&daemon, &run_id).await;
    assert_eq!(
        resp.status(),
        200,
        "shell POST on a failed run should succeed"
    );
    let body = resp.json::<serde_json::Value>().await.unwrap();
    let expected = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);
    assert_eq!(body["session"].as_str(), Some(expected.as_str()));
    assert_eq!(
        body["created"],
        serde_json::json!(true),
        "first open creates"
    );
    assert_eq!(body["ok"], serde_json::json!(true));

    let socket = daemon.tmux_socket();
    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &expected),
        "the tmux session must exist after opening the shell"
    );
}

#[tokio::test]
async fn open_shell_is_idempotent() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;

    let first = post_shell(&daemon, &run_id)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(first["created"], serde_json::json!(true));

    let second = post_shell(&daemon, &run_id).await;
    assert_eq!(second.status(), 200);
    let second = second.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        second["created"],
        serde_json::json!(false),
        "a second open re-attaches the existing shell"
    );

    let socket = daemon.tmux_socket();
    assert_eq!(
        shell_session_count(&socket),
        1,
        "exactly one shell session regardless of the number of opens"
    );
}

#[tokio::test]
async fn shell_runs_real_bash_in_pipeline_worktree() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;

    let resp = post_shell(&daemon, &run_id).await;
    assert_eq!(resp.status(), 200);
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);
    let socket = daemon.tmux_socket();

    // A real bash — not the `sleep 600` test override — in the pipeline worktree.
    // Prove cwd + writability, and that the env-safety export is inherited.
    pdo_daemon::tmux_session_manager::send_keys(
        &socket,
        &session,
        "echo PDO_OK > fp316-marker.txt",
    );
    pdo_daemon::tmux_session_manager::send_keys(
        &socket,
        &session,
        "echo \"$CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC\" > env-marker.txt",
    );

    let worktree = worktree_path(&daemon, &run_id);
    let marker = worktree.join("fp316-marker.txt");
    let env_marker = worktree.join("env-marker.txt");

    // Poll for the side effect (interactive bash + send-keys is async).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if marker.exists() && env_marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let got = std::fs::read_to_string(&marker).unwrap_or_else(|_| {
        panic!(
            "marker must exist at {} — proves cwd = worktree and real bash",
            marker.display()
        )
    });
    assert!(got.contains("PDO_OK"), "marker bytes: {got}");
    let env_got = std::fs::read_to_string(&env_marker).unwrap_or_default();
    assert_eq!(
        env_got.trim(),
        "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC must be exported =1 in the shell"
    );
}

#[tokio::test]
async fn shell_survives_reaper_for_terminal_run() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;

    post_shell(&daemon, &run_id).await;
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);
    let socket = daemon.tmux_socket();
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    // A non-zero (default) TTL sweep must NOT reap the shell of a live terminal
    // run — the Shell arm reaps only on absent/archived, never a TTL.
    daemon.run_orphan_sweep_tick().await;
    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "shell of a terminal, non-archived run must survive the reaper"
    );
}

#[tokio::test]
async fn reaper_kills_shell_of_absent_run() {
    // A `pdo-shell-<run>` whose run is absent from the event log (never created)
    // is an orphan → the sweep kills it (mirror of the Manager "absent" arm).
    let daemon = TestDaemon::spawn(seed_plain(LIVE_YAML, LIVE_PIPELINE))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let bogus_run = "20200101-000000-deadbee";
    let session = pdo_daemon::tmux_session_manager::shell_session_name(bogus_run);
    pdo_daemon::tmux_session_manager::spawn_shell(
        &session,
        daemon.repo_root(),
        bogus_run,
        daemon.addr.port(),
        None,
    )
    .unwrap();
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    daemon.run_orphan_sweep_tick().await;
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "a shell for an absent run must be reaped"
    );
}

#[tokio::test]
async fn reaper_kills_shell_of_archived_run() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);

    // Archive the run (cleanup_run). It kills the shell + removes the worktree.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "cleanup_run should archive");
    assert!(wait_for_run_status(&daemon, &run_id, "archived").await);

    // Re-spawn a shell session for the now-archived run out-of-band, then sweep:
    // the Shell arm resolves is_archived=true → reap.
    pdo_daemon::tmux_session_manager::spawn_shell(
        &session,
        daemon.repo_root(),
        &run_id,
        daemon.addr.port(),
        None,
    )
    .unwrap();
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));
    daemon.run_orphan_sweep_tick().await;
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "a shell for an archived run must be reaped"
    );
}

#[tokio::test]
async fn cleanup_run_kills_the_shell() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);

    post_shell(&daemon, &run_id).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "cleanup_run must kill the run shell before tearing down the worktree"
    );
}

#[tokio::test]
async fn resume_run_kills_the_shell() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);

    post_shell(&daemon, &run_id).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    // Resume the failed run — the interlock kills the shell (best-effort) before
    // re_evaluate_after_command re-drives the merge.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "resume_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut killed = false;
    while std::time::Instant::now() < deadline {
        if !pdo_daemon::tmux_session_manager::session_exists(&socket, &session) {
            killed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        killed,
        "resume_run must kill the open shell (best-effort interlock)"
    );
}

#[tokio::test]
async fn gate_rejects_live_run() {
    // A live (running) run must be refused: a stray edit in its worktree would
    // break the daemon's git merge on node completion.
    let daemon = TestDaemon::spawn(seed_plain(LIVE_YAML, LIVE_PIPELINE))
        .await
        .unwrap();
    let run_id = start_run(&daemon, LIVE_PIPELINE).await;
    assert!(
        wait_for_run_status(&daemon, &run_id, "running").await,
        "the sleep-override worker keeps the run running"
    );

    let resp = post_shell(&daemon, &run_id).await;
    assert_eq!(resp.status(), 409, "a live run is not shell-eligible");
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "no shell session may be spawned for a rejected live run"
    );
}

#[tokio::test]
async fn gate_rejects_archived_run() {
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    assert!(wait_for_run_status(&daemon, &run_id, "archived").await);

    let resp = post_shell(&daemon, &run_id).await;
    assert_eq!(resp.status(), 409, "an archived run is not shell-eligible");
}

#[tokio::test]
async fn gate_returns_404_for_unknown_run() {
    let daemon = TestDaemon::spawn(seed_plain(LIVE_YAML, LIVE_PIPELINE))
        .await
        .unwrap();
    let resp = post_shell(&daemon, "20200101-000000-nosuchr").await;
    assert_eq!(resp.status(), 404, "an unknown run must 404");
}

#[tokio::test]
async fn shell_survives_pty_client_disconnect() {
    // Integration smoke test for ADR-0021 #4 through the *real* PTY bridge:
    // attach a WS client to `/sessions/<session>/pty` exactly as the browser
    // modal does, close it cleanly, and assert the session is still alive.
    //
    // NB: whether a clean WS close actually delivers an EOF to the pane (and so
    // kills a bare `bash -i`) is environment-dependent — the validation env
    // reproduced it, this test env does not. So this test is NOT the regression
    // discriminator; `shell_survives_eof_and_exit` is (it forces the exact death
    // mechanism deterministically). This one guards that the end-to-end bridge
    // cycle doesn't itself tear the session down.
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);

    post_shell(&daemon, &run_id).await;
    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "shell must exist right after opening"
    );

    // Attach a real PTY-bridge client, exactly as the browser modal does.
    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("PTY WS should connect");
    // Drive one resize + a keystroke, then read whatever the pane emits so the
    // bridge is genuinely live before we tear it down.
    use futures_util::{SinkExt, StreamExt};
    ws.send(WsMessage::Text(
        r#"{"type":"resize","cols":100,"rows":30}"#.into(),
    ))
    .await
    .unwrap();
    ws.send(WsMessage::Binary(b"echo hi\n".to_vec().into()))
        .await
        .unwrap();
    // Read a couple of output frames (best-effort, bounded).
    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }

    // Close the WS cleanly — the "close the modal / tab" event.
    ws.close(None).await.ok();
    drop(ws);

    // Give the daemon time to tear the bridge down.
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "shell session must SURVIVE the PTY client disconnecting (ADR-0021 #4)"
    );
}

#[tokio::test]
async fn shell_survives_eof_and_exit() {
    // The deterministic regression guard for iteration 1's persistence bug.
    //
    // Root cause (from validation): a bare `bash -i` exits on EOF — a stray
    // Ctrl-D, an explicit `exit`, or the PTY bridge feeding EOF to the pane when
    // the modal/tab closes. Being the session's ONLY window, that exit destroys
    // the whole tmux session (ADR-0021 #4 violated). Feeding a raw Ctrl-D to the
    // pane reproduces the exact failure independently of any environment-specific
    // bridge timing: on the old tail the session is gone here; on the fixed
    // respawn-loop tail it survives and stays usable.
    ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(seed_script(
        FAIL_YAML,
        FAIL_PIPELINE,
        "#!/usr/bin/env bash\nexit 1\n",
    ))
    .await
    .unwrap();
    let run_id = failed_run(&daemon).await;
    let socket = daemon.tmux_socket();
    let session = pdo_daemon::tmux_session_manager::shell_session_name(&run_id);

    post_shell(&daemon, &run_id).await;
    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "shell must exist right after opening"
    );

    // Hammer the pane with the exit triggers that killed iteration 1's shell:
    // several EOFs (Ctrl-D) and an explicit `exit`.
    let send_raw = |keys: &str| {
        let _ = Command::new("tmux")
            .args(["-L", &socket, "send-keys", "-t", &session, keys])
            .output();
    };
    for _ in 0..3 {
        send_raw("C-d");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    pdo_daemon::tmux_session_manager::send_keys(&socket, &session, "exit");
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "shell session must SURVIVE EOF/exit — its interactive bash is respawned \
         so the pane (and session) outlives any single shell (ADR-0021 #4)"
    );

    // ...and a fresh, usable bash must have taken its place: prove it by writing
    // a marker from the respawned shell into the pipeline worktree.
    pdo_daemon::tmux_session_manager::send_keys(
        &socket,
        &session,
        "echo RESPAWN_OK > respawn-marker.txt",
    );
    let marker = worktree_path(&daemon, &run_id).join("respawn-marker.txt");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let got = std::fs::read_to_string(&marker).unwrap_or_else(|_| {
        panic!(
            "respawned shell must be usable — marker missing at {}",
            marker.display()
        )
    });
    assert!(got.contains("RESPAWN_OK"), "marker bytes: {got}");
}
