//! Layer 3a — library pipeline authoring assistant (#302 / ADR-0048, reshaped by
//! #594 / ADR-0051).
//!
//! Drives `POST` / `DELETE /sessions/libassist` and `PUT` / `GET
//! /sessions/libassist/focus` against a real daemon, and corroborates every side
//! effect out-of-band on the daemon's private tmux socket and on disk.
//!
//! The assistant is a `claude` REPL, but here it runs the daemon's harmless
//! `exec sleep 600` tmux override (the `Agent` tail honours the seam), so the
//! session is created and persists without launching a real `claude`. Everything
//! resolves under the test's tempdir repo root — no `$HOME` dependency, and no
//! pipeline file needs to pre-exist.
//!
//! **Every daemon here is `spawn_nested`** (same reason as `manager_pty.rs`): the
//! assistant is now genuinely reapable, so a *background* reaper tick would race
//! the test — and `PDO_REAPER_INTERVAL_SECS` is process-global, so a concurrent
//! test file can shrink that interval under our feet. Opting out of the automatic
//! loop leaves `run_orphan_sweep_tick` as the only sweep, which is exactly the
//! determinism the reap assertions need.

use std::time::Duration;

use crate::common::TestDaemon;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn session_name() -> &'static str {
    pdo_daemon::tmux_session_manager::libassist_session_name()
}

async fn post_assistant(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/sessions/libassist", daemon.url()))
        .send()
        .await
        .unwrap()
}

async fn delete_assistant(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::Client::new()
        .delete(format!("{}/sessions/libassist", daemon.url()))
        .send()
        .await
        .unwrap()
}

async fn put_focus(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .put(format!("{}/sessions/libassist/focus", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn get_focus(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/sessions/libassist/focus", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// Number of `pdo-libassist-*` sessions on the daemon's socket.
fn assistant_session_count(socket: &str) -> usize {
    pdo_daemon::tmux_session_manager::list_pdo_sessions(socket)
        .into_iter()
        .filter(|s| s.starts_with("pdo-libassist-"))
        .count()
}

/// Write a template into the repo store so a focus on it resolves to a real path.
fn seed_repo_template(daemon: &TestDaemon, id: &str) -> std::path::PathBuf {
    let dir = daemon.repo_root().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.yaml"));
    std::fs::write(&path, "name: seeded\nnodes: []\nedges: []\n").unwrap();
    path
}

#[tokio::test]
async fn open_assistant_creates_session() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();

    let resp = post_assistant(&daemon).await;
    assert_eq!(resp.status(), 200, "opening the assistant should succeed");
    let body = resp.json::<serde_json::Value>().await.unwrap();

    assert_eq!(body["session"].as_str(), Some(session_name()));
    assert_eq!(
        body["created"],
        serde_json::json!(true),
        "first open creates"
    );
    assert_eq!(body["ok"], serde_json::json!(true));

    let socket = daemon.tmux_socket();
    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, session_name()),
        "the tmux session must exist after opening the assistant"
    );

    // #594: the cwd is the repo's TEMPLATES dir (`.pdo/pipelines`), not the
    // library store. That mismatch is what used to launch the assistant in a
    // folder where the file its primer named did not exist.
    let pdo = daemon.repo_root().join(".pdo");
    assert!(
        pdo.join("pipelines").is_dir(),
        "the templates dir (the assistant's cwd) is created"
    );

    // Primer AND hook settings are written out of the user-facing dir, with no
    // pipeline id in either name — there is one assistant.
    assert!(
        pdo.join(".libassist").join("assistant.md").is_file(),
        "the primer is written to a sibling .libassist dir, not into pipelines/"
    );
    assert!(
        pdo.join(".libassist").join("settings.json").is_file(),
        "the UserPromptSubmit hook settings ship beside the primer"
    );
}

/// The hook is the whole per-message-awareness mechanism (ADR-0051 §3), so what
/// lands on disk is checked, not just that a file exists: a `UserPromptSubmit`
/// entry, and a command that can never reject the user's prompt.
#[tokio::test]
async fn open_assistant_arms_a_non_blocking_focus_hook() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    post_assistant(&daemon).await;

    let raw = std::fs::read_to_string(
        daemon
            .repo_root()
            .join(".pdo")
            .join(".libassist")
            .join("settings.json"),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).expect("claude parses this at launch");
    let cmd = v["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .expect("a UserPromptSubmit command hook");
    assert!(
        cmd.contains("/sessions/libassist/focus"),
        "the hook fetches the daemon's focus: {cmd}"
    );
    assert!(
        cmd.trim_end().ends_with("exit 0"),
        "a UserPromptSubmit hook exiting non-zero would erase the user's message: {cmd}"
    );
}

#[tokio::test]
async fn open_assistant_is_idempotent() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();

    let first = post_assistant(&daemon)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(first["created"], serde_json::json!(true));

    let second = post_assistant(&daemon).await;
    assert_eq!(second.status(), 200);
    let second = second.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        second["created"],
        serde_json::json!(false),
        "a second open re-attaches the existing assistant"
    );

    assert_eq!(
        assistant_session_count(&daemon.tmux_socket()),
        1,
        "exactly one assistant session regardless of the number of opens"
    );
}

/// **The sharing property** (#594, point 3 of the issue). Two opens made while
/// two *different* templates are focused — yesterday two separate sessions —
/// resolve to the same session, so the conversation survives the round trip.
#[tokio::test]
async fn one_assistant_is_shared_across_templates() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    seed_repo_template(&daemon, "alpha");
    seed_repo_template(&daemon, "beta");

    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;
    let first = post_assistant(&daemon)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();

    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "beta", "scope": "repo"}),
    )
    .await;
    let second = post_assistant(&daemon)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(
        first["session"], second["session"],
        "the same session serves both templates"
    );
    assert_eq!(
        second["created"],
        serde_json::json!(false),
        "switching template must NOT respawn — that would throw away the conversation"
    );
    assert_eq!(assistant_session_count(&daemon.tmux_socket()), 1);
}

#[tokio::test]
async fn close_assistant_reaps_the_session() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    post_assistant(&daemon).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket,
        session_name()
    ));

    let resp = delete_assistant(&daemon).await;
    assert_eq!(resp.status(), 200);
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["ok"], serde_json::json!(true));
    assert_eq!(
        body["reaped"],
        serde_json::json!(true),
        "leaving every edit view reaps the live session"
    );
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, session_name()),
        "the tmux session is gone after leave"
    );

    // Reaping again is a harmless no-op (double-leave / race).
    let second = delete_assistant(&daemon)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        second["reaped"],
        serde_json::json!(false),
        "a second leave finds nothing to reap"
    );
}

// ---------------------------------------------------------------------------
// The focus (#594, ADR-0051 §2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn focus_round_trips_and_resolves_the_absolute_path() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let path = seed_repo_template(&daemon, "alpha");

    assert_eq!(
        get_focus(&daemon).await["pipeline_id"],
        serde_json::json!(null),
        "no focus before the UI declares one"
    );

    let put = put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;
    assert_eq!(put.status(), 200);

    let focus = get_focus(&daemon).await;
    assert_eq!(focus["pipeline_id"], serde_json::json!("alpha"));
    assert_eq!(focus["scope"], serde_json::json!("repo"));
    // The CLIENT never sends a path — the daemon resolves it, because the two
    // meanings of "scope" (edit tab vs library store) do not coincide.
    assert_eq!(
        focus["path"],
        serde_json::json!(path.to_string_lossy()),
        "the daemon resolves the absolute path itself"
    );
    assert!(focus["age_secs"].as_i64().unwrap() < 5);
}

#[tokio::test]
async fn focus_accepts_an_unsaved_template_with_a_null_path() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();

    let resp = put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "never-saved", "scope": "repo"}),
    )
    .await;
    assert_eq!(
        resp.status(),
        200,
        "a template open in the canvas but not yet on disk is a legitimate focus"
    );

    let focus = get_focus(&daemon).await;
    assert_eq!(focus["pipeline_id"], serde_json::json!("never-saved"));
    assert_eq!(focus["path"], serde_json::json!(null));
}

#[tokio::test]
async fn focus_rejects_an_unknown_scope() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let resp = put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "bogus"}),
    )
    .await;
    assert_eq!(resp.status(), 400, "an unknown scope is a 400");
    assert_eq!(
        get_focus(&daemon).await["pipeline_id"],
        serde_json::json!(null),
        "a rejected declaration stores nothing"
    );
}

#[tokio::test]
async fn a_null_pipeline_id_clears_the_focus() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    seed_repo_template(&daemon, "alpha");

    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;
    assert_eq!(
        get_focus(&daemon).await["pipeline_id"],
        serde_json::json!("alpha")
    );

    put_focus(&daemon, serde_json::json!({"pipeline_id": null})).await;
    let cleared = get_focus(&daemon).await;
    assert_eq!(cleared["pipeline_id"], serde_json::json!(null));
    assert_eq!(cleared["age_secs"], serde_json::json!(null));
}

/// `?format=text` is what the hook injects verbatim into the assistant's context.
/// It must name the pipeline, the scope, the absolute path — and warn about the
/// save default, which is the silent `user → repo` migration this issue also fixes.
#[tokio::test]
async fn focus_renders_a_plain_line_for_the_hook() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let path = seed_repo_template(&daemon, "alpha");
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    let text = reqwest::get(format!(
        "{}/sessions/libassist/focus?format=text",
        daemon.url()
    ))
    .await
    .unwrap()
    .text()
    .await
    .unwrap();

    assert!(text.contains("alpha"), "names the pipeline: {text}");
    assert!(text.contains("repo"), "names the scope: {text}");
    assert!(
        text.contains(&*path.to_string_lossy()),
        "names the absolute path: {text}"
    );
    assert!(text.contains("scope"), "warns about the save scope: {text}");
}

/// Declaring a focus must never *start* the assistant (ADR-0048 ruled auto-spawn
/// out, and ADR-0051 does not revisit it): the UI declares its focus on every edit
/// view, and spawning a `claude` for each would be both eager and expensive.
#[tokio::test]
async fn focus_never_spawns_the_assistant() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    seed_repo_template(&daemon, "alpha");

    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    assert_eq!(
        assistant_session_count(&daemon.tmux_socket()),
        0,
        "a focus declaration is not a request for a REPL"
    );
}

// ---------------------------------------------------------------------------
// The sweep (#594, ADR-0051 §4)
// ---------------------------------------------------------------------------

/// **Point 2 of the issue**: the user is editing (the UI re-declared its focus),
/// with no terminal attached — the state after the info panel auto-closes on an
/// edit-tab switch (#385). The session must survive a full sweep pass.
#[tokio::test]
async fn the_sweep_keeps_an_assistant_with_a_fresh_focus() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    seed_repo_template(&daemon, "alpha");

    post_assistant(&daemon).await;
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    daemon.run_orphan_sweep_tick().await;

    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, session_name()),
        "an assistant whose user is still editing must not be reaped"
    );
}

/// **The successor of `assistant_survives_the_orphan_sweep`, and it says the
/// opposite on purpose.** That test pinned ADR-0048's unconditional exemption,
/// which is what let a session leaked by a browser reload live unbounded: no
/// `DELETE` is ever sent (React runs no cleanup on unload) and nothing else
/// reaped it. Detached, no focus ever declared ⇒ the sweep takes it.
#[tokio::test]
async fn the_sweep_reaps_an_assistant_nobody_is_using() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    post_assistant(&daemon).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket,
        session_name()
    ));

    // No `PUT focus`, no attached terminal — exactly what a reload leaves behind.
    daemon.run_orphan_sweep_tick().await;

    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, session_name()),
        "an assistant with nobody attached and no declared focus is reaped"
    );
}

/// A stale focus does not beat an open terminal: someone reading the pane
/// declares no focus, and yanking their session would be the worst failure mode
/// of the three verdicts. Drives the real PTY bridge (a genuine `tmux attach`),
/// so `#{session_attached}` is exercised, not stubbed.
#[tokio::test]
async fn the_sweep_keeps_an_attached_assistant_with_no_focus() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    post_assistant(&daemon).await;

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect to the assistant PTY should succeed");
    // Let the bridge's `tmux attach` land before observing `session_attached`.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        pdo_daemon::tmux_session_manager::is_attached(&socket, session_name()),
        "precondition: the PTY bridge attached a real tmux client"
    );

    // No focus at all — the arm that reaps in the test above.
    daemon.run_orphan_sweep_tick().await;

    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, session_name()),
        "an attached terminal is never yanked out from under its reader"
    );

    let _ = futures_util::SinkExt::close(&mut ws).await;
}
