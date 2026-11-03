//! Layer 4 (ADR-0080) — the Apparence of each Actor (CONTEXT.md → *Apparence*)
//! at the daemon's HTTP seam: `GET` / `PUT /actors/me/appearance`, with the
//! internal test extension identifying two Actors by its header
//! (`x-pdo-test-actor`), as the identity tests do (`extension_identity`).
//!
//! - each Actor reads what it wrote, never another's;
//! - without an identity extension, every call is the Opérateur local's;
//! - a malformed or too large document is refused and stores nothing;
//! - the document survives a restart of the daemon.

use pdo::extension_api::Extension;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::common::{flow, TestDaemon};
use crate::test_extension::{Refusal, TestExtension, ACTOR_HEADER};

fn extensions() -> Vec<Box<dyn Extension>> {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    vec![Box::new(ext)]
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// `GET /actors/me/appearance` as `actor` (none: nobody identified).
async fn read(url: &str, actor: Option<&str>) -> Value {
    let mut request = client().get(format!("{url}/actors/me/appearance"));
    if let Some(actor) = actor {
        request = request.header(ACTOR_HEADER, actor);
    }
    let resp = request.send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json().await.unwrap()
}

/// `PUT /actors/me/appearance` of the raw `body` as `actor`.
async fn write(url: &str, actor: Option<&str>, body: String) -> (StatusCode, Value) {
    let mut request = client()
        .put(format!("{url}/actors/me/appearance"))
        .header("content-type", "application/json")
        .body(body);
    if let Some(actor) = actor {
        request = request.header(ACTOR_HEADER, actor);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn two_actors_each_keep_their_own_appearance() {
    let daemon = TestDaemon::spawn_with_extensions(
        |repo| flow::seed_solo(repo, &[("solo", false)]),
        extensions(),
    )
    .await
    .unwrap();
    let url = daemon.url();
    assert_eq!(
        read(&url, Some("alice")).await,
        json!({ "appearance": null })
    );

    let alice = json!({
        "version": 1, "theme": "classic", "dark_mode": false, "compact_nodes": true,
        "elements": { "runs.list.cost": true },
    });
    let (status, kept) = write(&url, Some("alice"), alice.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kept, json!({ "appearance": alice }));

    // Bob has nothing yet, then his own; Alice's is untouched.
    assert_eq!(read(&url, Some("bob")).await, json!({ "appearance": null }));
    let bob = json!({ "version": 1, "elements": {} });
    let (status, _) = write(&url, Some("bob"), bob.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read(&url, Some("bob")).await, json!({ "appearance": bob }));
    assert_eq!(
        read(&url, Some("alice")).await,
        json!({ "appearance": alice })
    );
    // Nobody identified: the Opérateur local, who wrote nothing.
    assert_eq!(read(&url, None).await, json!({ "appearance": null }));
}

#[tokio::test]
async fn without_an_identity_extension_the_local_operator_reads_what_it_wrote() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let url = daemon.url();
    assert_eq!(read(&url, None).await, json!({ "appearance": null }));
    let document = json!({ "version": 1, "dark_mode": false, "elements": {} });
    let (status, _) = write(&url, None, document.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read(&url, None).await, json!({ "appearance": document }));
}

#[tokio::test]
async fn a_malformed_or_too_large_document_is_refused() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let url = daemon.url();
    let kept = json!({ "version": 1, "compact_nodes": true, "elements": {} });
    assert_eq!(write(&url, None, kept.to_string()).await.0, StatusCode::OK);

    for body in [
        "not json".to_string(),
        json!({ "theme": "classic" }).to_string(),
        json!({ "version": 1, "dark_mode": "yes" }).to_string(),
        json!({ "version": 1, "elements": { "runs.list.cost": 0 } }).to_string(),
        json!({ "version": 1, "unknown": true }).to_string(),
    ] {
        let (status, refused) = write(&url, None, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            refused["error"]
                .as_str()
                .unwrap()
                .starts_with("invalid appearance"),
            "{refused}"
        );
    }
    let elements: serde_json::Map<String, Value> = (0..2000)
        .map(|i| (format!("runs.list.element-{i:05}"), Value::Bool(false)))
        .collect();
    let huge = json!({ "version": 1, "elements": elements }).to_string();
    let (status, refused) = write(&url, None, huge).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        refused["error"].as_str().unwrap().contains("bytes"),
        "{refused}"
    );

    assert_eq!(read(&url, None).await, json!({ "appearance": kept }));
}

#[tokio::test]
async fn the_appearance_survives_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let document = json!({ "version": 1, "theme": "classic", "elements": {} });
    let first = TestDaemon::spawn_at_with_extensions(root.path(), extensions())
        .await
        .unwrap();
    let (status, _) = write(&first.url(), Some("alice"), document.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    drop(first);

    let second = TestDaemon::spawn_at_with_extensions(root.path(), extensions())
        .await
        .unwrap();
    assert_eq!(
        read(&second.url(), Some("alice")).await,
        json!({ "appearance": document })
    );
}
