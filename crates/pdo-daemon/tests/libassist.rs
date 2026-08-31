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

/// Leaving is **one** gesture, not two. The `pagehide` path can only afford a
/// single `keepalive` request, so a focus cleared by a separate call was never
/// cleared at all there: `GET …/focus` kept naming a template nobody had open,
/// with an age growing without bound, and a second browser tab would have
/// inherited it.
#[tokio::test]
async fn close_assistant_clears_the_focus() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    seed_repo_template(&daemon, "alpha");
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    delete_assistant(&daemon).await;

    let focus = get_focus(&daemon).await;
    assert_eq!(
        focus["pipeline_id"],
        serde_json::json!(null),
        "the reap clears the focus by the same gesture"
    );
    assert_eq!(focus["age_secs"], serde_json::json!(null));
}

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
    assert!(focus.get("scope").is_none());
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
async fn focus_ignores_the_retired_scope_field() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let resp = put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "bogus"}),
    )
    .await;
    assert_eq!(resp.status(), 200, "scope no longer selects a registry");
    assert_eq!(
        get_focus(&daemon).await["pipeline_id"],
        serde_json::json!("alpha"),
        "the instance pipeline identity is retained"
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
/// It must name the pipeline, the absolute path — and the one endpoint
/// that writes where the focus points. Naming the save endpoint here is not
/// decoration: the assistant reads this line before every message, and pointing it
/// at `POST /library/pipelines` is what made it write a duplicate into the wrong
/// store while announcing a save.
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
    assert!(
        !text.contains("scope `"),
        "does not expose a retired scope: {text}"
    );
    assert!(
        text.contains(&*path.to_string_lossy()),
        "names the absolute path: {text}"
    );
    assert!(
        text.contains("/sessions/libassist/save"),
        "names the one endpoint that writes where the focus points: {text}"
    );
    assert!(!text.contains("/library/pipelines"));
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

/// A three-node template, valid enough for the parser and different enough from
/// the seeded one that "did it write?" is unambiguous.
const SAVED_YAML: &str = r#"name: alpha
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: gamma
    name: GAMMA
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: gamma, port: task }
"#;

async fn post_save(daemon: &TestDaemon, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/sessions/libassist/save", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// **The FP-6 regression.** The assistant used to persist through
/// `POST /library/pipelines`, echoing the focus scope back at it — but that
/// endpoint reads `scope` in the *library store's* vocabulary. A `repo` template
/// "saved" that way landed in `.pdo/library/pipelines/` as a duplicate, the edited
/// file never moved, the canvas never changed, and the assistant reported success.
///
/// Both halves are asserted, because only the pair is the property: the edited
/// file changed, **and** no copy appeared in the other store.
#[tokio::test]
async fn save_writes_the_focused_template_in_place() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let path = seed_repo_template(&daemon, "alpha");
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    let resp = post_save(&daemon, serde_json::json!({"yaml": SAVED_YAML})).await;
    assert_eq!(resp.status(), 200, "saving the open template succeeds");
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["id"], serde_json::json!("alpha"));
    assert!(body.get("scope").is_none());
    assert_eq!(body["path"], serde_json::json!(path.to_string_lossy()));

    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        on_disk, SAVED_YAML,
        "the edited file is the one that changed"
    );

    let library_dir = daemon
        .repo_root()
        .join(".pdo")
        .join("library")
        .join("pipelines");
    assert!(
        !library_dir.join("alpha.yaml").exists(),
        "no duplicate in the library store — that was the whole bug"
    );
}

/// Node prompts ride along the YAML, under the canonical `<id>.prompts/` sibling
/// the rest of the codebase reads.
#[tokio::test]
async fn save_writes_node_prompts_beside_the_template() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let path = seed_repo_template(&daemon, "alpha");
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    let resp = post_save(
        &daemon,
        serde_json::json!({"yaml": SAVED_YAML, "prompts": {"gamma": "You are GAMMA."}}),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let prompt = path.with_extension("prompts").join("gamma.md");
    assert_eq!(
        std::fs::read_to_string(&prompt).unwrap(),
        "You are GAMMA.",
        "the node prompt lands next to the template"
    );
}

#[tokio::test]
async fn save_ignores_a_retired_library_scope() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let library_dir = daemon
        .repo_root()
        .join(".pdo")
        .join("library")
        .join("pipelines");
    std::fs::create_dir_all(&library_dir).unwrap();
    let library_path = library_dir.join("alpha.yaml");
    std::fs::write(&library_path, "name: seeded\nnodes: []\nedges: []\n").unwrap();

    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "library"}),
    )
    .await;

    let resp = post_save(&daemon, serde_json::json!({"yaml": SAVED_YAML})).await;
    assert_eq!(resp.status(), 200);

    assert_eq!(
        std::fs::read_to_string(&library_path).unwrap(),
        "name: seeded\nnodes: []\nedges: []\n",
        "the legacy library is not a write target"
    );
    assert_eq!(
        std::fs::read_to_string(
            daemon
                .repo_root()
                .join(".pdo")
                .join("pipelines")
                .join("alpha.yaml")
        )
        .unwrap(),
        SAVED_YAML,
        "the instance registry receives the save"
    );
}

/// Saving into a guess is exactly what the focus mechanism exists to prevent, so
/// "no template open" is a refusal, not a default.
#[tokio::test]
async fn save_without_a_focus_is_refused() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();

    let resp = post_save(&daemon, serde_json::json!({"yaml": SAVED_YAML})).await;
    assert_eq!(resp.status(), 409, "nothing open ⇒ nothing to save into");
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("open"),
        "the refusal says why: {body}"
    );
}

/// Same gate as `PUT /pipelines/{id}`: unparseable YAML must not reach the disk
/// the canvas is reading from.
#[tokio::test]
async fn save_rejects_yaml_that_does_not_parse() {
    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    let path = seed_repo_template(&daemon, "alpha");
    let before = std::fs::read_to_string(&path).unwrap();
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    let resp = post_save(&daemon, serde_json::json!({"yaml": "nodes: [unclosed"})).await;
    assert_eq!(resp.status(), 400);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "a rejected save leaves the file alone"
    );
}

/// The canvas re-reads on save, and it learns about the write from the daemon —
/// not from the file watcher, which is suppressed for the daemon's own writes and
/// does not watch the library store at all. Without this event the user sees the
/// assistant announce a save and the canvas sit unchanged, which reads exactly
/// like the bug that is being fixed.
#[tokio::test]
async fn save_tells_the_canvas_to_re_read() {
    use futures_util::StreamExt;

    let daemon = TestDaemon::spawn_nested(|_| Ok(())).await.unwrap();
    seed_repo_template(&daemon, "alpha");
    put_focus(
        &daemon,
        serde_json::json!({"pipeline_id": "alpha", "scope": "repo"}),
    )
    .await;

    let mut ws = daemon.connect_ws().await.unwrap();
    post_save(&daemon, serde_json::json!({"yaml": SAVED_YAML})).await;

    let found = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let msg = ws.next().await?.ok()?;
            let text = crate::common::ws_text(&msg)?.to_string();
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            if value["type"] == serde_json::json!("pipeline_changed")
                && value["pipeline_id"] == serde_json::json!("alpha")
            {
                return Some(value);
            }
        }
    })
    .await
    .ok()
    .flatten();

    assert!(
        found.is_some(),
        "the save broadcasts pipeline_changed for the template it wrote"
    );
}
