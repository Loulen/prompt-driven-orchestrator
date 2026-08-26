//! Layer 3a — library pipeline authoring assistant (#302 / ADR-0048).
//!
//! Drives `POST` / `DELETE /sessions/{pipeline_id}/libassist` against a real
//! daemon and corroborates every side effect out-of-band on the daemon's private
//! tmux socket + the library pipelines directory on disk.
//!
//! The assistant is a `claude` REPL, but here it runs the daemon's harmless
//! `exec sleep 600` tmux override (the `Agent` tail honours the seam), so the
//! session is created and persists without launching a real `claude`. We drive
//! `?scope=repo` so the cwd is the tempdir's `.pdo/library/pipelines` — fully
//! hermetic, no `$HOME` dependency, and no pipeline file needs to pre-exist
//! (create-if-absent creates the directory).

use crate::common::TestDaemon;

const PIPELINE_ID: &str = "feature-with-review";

async fn post_assistant(daemon: &TestDaemon, id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!(
            "{}/sessions/{id}/libassist?scope=repo",
            daemon.url()
        ))
        .send()
        .await
        .unwrap()
}

async fn delete_assistant(daemon: &TestDaemon, id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .delete(format!("{}/sessions/{id}/libassist", daemon.url()))
        .send()
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

#[tokio::test]
async fn open_assistant_creates_session() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();

    let resp = post_assistant(&daemon, PIPELINE_ID).await;
    assert_eq!(resp.status(), 200, "opening the assistant should succeed");
    let body = resp.json::<serde_json::Value>().await.unwrap();

    let expected = pdo_daemon::tmux_session_manager::libassist_session_name(PIPELINE_ID);
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
        "the tmux session must exist after opening the assistant"
    );

    // The cwd (the repo-scoped pipelines dir) is created on demand so the very
    // first template can be authored, and the primer is written OUT of it.
    let lib = daemon.repo_root().join(".pdo").join("library");
    assert!(
        lib.join("pipelines").is_dir(),
        "the pipelines dir (the assistant's cwd) is created"
    );
    assert!(
        lib.join(".libassist")
            .join(format!("{PIPELINE_ID}.md"))
            .is_file(),
        "the primer is written to a sibling .libassist dir, not into pipelines/"
    );
}

#[tokio::test]
async fn open_assistant_is_idempotent() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();

    let first = post_assistant(&daemon, PIPELINE_ID)
        .await
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(first["created"], serde_json::json!(true));

    let second = post_assistant(&daemon, PIPELINE_ID).await;
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

#[tokio::test]
async fn close_assistant_reaps_the_session() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let session = pdo_daemon::tmux_session_manager::libassist_session_name(PIPELINE_ID);
    let socket = daemon.tmux_socket();

    post_assistant(&daemon, PIPELINE_ID).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    let resp = delete_assistant(&daemon, PIPELINE_ID).await;
    assert_eq!(resp.status(), 200);
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["ok"], serde_json::json!(true));
    assert_eq!(
        body["reaped"],
        serde_json::json!(true),
        "the leave reaps the live session"
    );
    assert!(
        !pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "the tmux session is gone after leave"
    );

    // Reaping again is a harmless no-op (double-leave / race).
    let second = delete_assistant(&daemon, PIPELINE_ID)
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

#[tokio::test]
async fn assistant_survives_the_orphan_sweep() {
    // The F3 correctness point (ADR-0048): a `pdo-libassist-*` session owns no
    // Run, so a naive sweep would read its name as unrecognised and kill it within
    // 60 s. The `libassist-` parse branch + the always-keep sweep arm prevent that.
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let session = pdo_daemon::tmux_session_manager::libassist_session_name(PIPELINE_ID);
    let socket = daemon.tmux_socket();

    post_assistant(&daemon, PIPELINE_ID).await;
    assert!(pdo_daemon::tmux_session_manager::session_exists(
        &socket, &session
    ));

    // A full orphan-sweep pass (the same one that reaps unrecognised names).
    daemon.run_orphan_sweep_tick().await;

    assert!(
        pdo_daemon::tmux_session_manager::session_exists(&socket, &session),
        "the assistant survives the sweep — it is reaped only on explicit leave"
    );
}

#[tokio::test]
async fn unknown_scope_is_rejected() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/sessions/{PIPELINE_ID}/libassist?scope=bogus",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "an unknown scope is a 400");
    assert_eq!(
        assistant_session_count(&daemon.tmux_socket()),
        0,
        "no session is created on a rejected scope"
    );
}
