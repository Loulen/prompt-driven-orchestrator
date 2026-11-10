//! API 0.11 against the real daemon, with the test extension: an extension's
//! event guarded by a read, and the `operation_applied` on an extension's
//! Ressource, reach on `/ws` only the connections whose Actor may read the
//! entity, in its Projet.
//!
//! - the test extension refuses Alice the read of its notes in P2: Alice's
//!   connection receives neither the `note_changed` nor the
//!   `operation_applied` of a note of P2, and still those of P1; Bob's
//!   connection receives them all;
//! - the guard never leaves the daemon;
//! - without a refusal, every connection receives everything.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::common::flow::git_init;
use crate::common::{ws_text, TestDaemon};
use crate::test_extension::{Refusal, TestExtension};

async fn send(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Value,
    actor: &str,
) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .request(method, format!("{}{path}", daemon.url()))
        .header("x-pdo-test-actor", actor)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// The Projet named `name` owning `repo`: its id.
async fn projet(daemon: &TestDaemon, name: &str, repo: &str) -> String {
    let (status, project) = send(
        daemon,
        reqwest::Method::POST,
        "/projects",
        json!({ "name": name, "members": [repo] }),
        "admin",
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

/// Bob updates the note `id` of the Projet `project` through the test
/// extension.
async fn update_note(daemon: &TestDaemon, id: &str, project: &str) {
    let (status, body) = send(
        daemon,
        reqwest::Method::PUT,
        &format!("/ext/test/notes/{id}"),
        json!({ "project": project, "title": format!("title of {id}") }),
        "bob",
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

/// What `/ws` carried of the notes within `window`: the raw text of each
/// `note_changed` and `operation_applied` on a note.
async fn note_messages<S>(ws: &mut S, window: Duration) -> Vec<String>
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Some(text) = ws_text(&message) {
            if text.contains("note-") {
                seen.push(text.to_string());
            }
        }
    }
    seen
}

/// `(kind, note id)` of each message: `note_changed` or `operation_applied`.
fn about(messages: &[String]) -> Vec<(String, String)> {
    let mut about: Vec<(String, String)> = messages
        .iter()
        .map(|text| {
            let message: Value = serde_json::from_str(text).unwrap();
            match message["type"].as_str().unwrap() {
                "extension_event" => (
                    message["name"].as_str().unwrap().to_string(),
                    message["payload"]["id"].as_str().unwrap().to_string(),
                ),
                kind => (
                    kind.to_string(),
                    message["operation"]["target"].as_str().unwrap().to_string(),
                ),
            }
        })
        .collect();
    about.sort();
    about
}

fn all_of(ids: &[&str]) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = ids
        .iter()
        .flat_map(|id| {
            [
                ("note_changed".to_string(), id.to_string()),
                ("operation_applied".to_string(), id.to_string()),
            ]
        })
        .collect();
    all.sort();
    all
}

#[tokio::test]
async fn ws_relays_an_extensions_guarded_event_and_applied_operation_only_to_who_may_read_them() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    git_init(other.path()).unwrap();
    let p1 = projet(&daemon, "P1", &daemon.target_repo()).await;
    let p2 = projet(&daemon, "P2", &other.path().to_string_lossy()).await;
    control.set(Refusal::parse(&format!("test/note:read@{p2}~alice")).unwrap());

    let mut alice = daemon.connect_ws_as("alice").await.unwrap();
    let mut bob = daemon.connect_ws_as("bob").await.unwrap();
    update_note(&daemon, "note-1", &p1).await;
    update_note(&daemon, "note-2", &p2).await;

    let window = Duration::from_secs(2);
    let (to_alice, to_bob) = tokio::join!(
        note_messages(&mut alice, window),
        note_messages(&mut bob, window)
    );
    assert_eq!(
        about(&to_alice),
        all_of(&["note-1"]),
        "nothing of P2 reaches Alice: {to_alice:?}"
    );
    assert_eq!(about(&to_bob), all_of(&["note-1", "note-2"]), "{to_bob:?}");
    for text in to_alice.iter().chain(&to_bob) {
        assert!(!text.contains("read_guard"), "the guard leaked: {text}");
    }

    // The read was asked as the read of the note, in its Projet.
    assert!(control.asked().iter().any(|(_, op)| {
        op.is_read()
            && op.resource.wire_name() == "test/note"
            && op.target.as_deref() == Some("note-2")
            && op.projects == [p2.clone()]
    }));
}

#[tokio::test]
async fn without_a_refusal_every_connection_receives_the_guarded_events() {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    let p1 = projet(&daemon, "P1", &daemon.target_repo()).await;

    let mut alice = daemon.connect_ws_as("alice").await.unwrap();
    update_note(&daemon, "note-1", &p1).await;

    let to_alice = note_messages(&mut alice, Duration::from_secs(1)).await;
    assert_eq!(about(&to_alice), all_of(&["note-1"]), "{to_alice:?}");
    assert!(to_alice.iter().all(|text| !text.contains("read_guard")));
}
