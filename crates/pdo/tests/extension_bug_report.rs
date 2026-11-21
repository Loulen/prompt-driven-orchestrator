//! Layer 4 (ADR-0080) — the Destinations de bug report of the extension API
//! 0.28 at the daemon's HTTP seam, with the internal test extension
//! (`test_extension/bug_reports.rs`) identifying its Actors by its header, as
//! the identity tests do (`extension_identity`):
//!
//! - `GET /bug-report/destinations`: what each extension offers the caller,
//!   under its name, and the caller's last choice;
//! - `POST /bug-report`: the report reaches the extension (its text, its
//!   fields, the screenshot, the prompts where they may go), which answers
//!   the item it created; the destination becomes the caller's last choice;
//!   refused without a description, with prompts toward a destination that
//!   refuses them, toward a destination not offered to the caller or toward
//!   the core's; the extension's own refusal reaches the caller;
//! - `PUT /bug-report/last`: the core's destination remembered, for one
//!   Actor only;
//! - `pdo bug-report`: the same, from the command line.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pdo::extension_api::Extension;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::cli_operator::pdo;
use crate::common::{flow, TestDaemon};
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

async fn daemon() -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let extensions: Vec<Box<dyn Extension>> = vec![Box::new(ext)];
    let daemon = TestDaemon::spawn_with_extensions(
        |repo| flow::seed_solo(repo, &[("solo", false)]),
        extensions,
    )
    .await
    .unwrap();
    (daemon, control)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn answer(resp: reqwest::Response) -> (StatusCode, Value) {
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// `GET /bug-report/destinations` as `actor`.
async fn destinations(url: &str, actor: &str) -> Value {
    let resp = client()
        .get(format!("{url}/bug-report/destinations"))
        .header(ACTOR_HEADER, actor)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json().await.unwrap()
}

/// `POST /bug-report` of `body` as `actor`.
async fn send(url: &str, actor: &str, body: Value) -> (StatusCode, Value) {
    answer(
        client()
            .post(format!("{url}/bug-report"))
            .header(ACTOR_HEADER, actor)
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await
}

/// `PUT /bug-report/last` of `destination` as `actor`.
async fn remember(url: &str, actor: &str, destination: &str) -> (StatusCode, Value) {
    answer(
        client()
            .put(format!("{url}/bug-report/last"))
            .header(ACTOR_HEADER, actor)
            .json(&json!({ "destination": destination }))
            .send()
            .await
            .unwrap(),
    )
    .await
}

fn report(destination: &str) -> Value {
    json!({
        "destination": destination,
        "text": "### Description\nThe node stays running\n\n### Prompts\n#### Design\nDraw it",
        "fields": {
            "description": "The node stays running\nafter complete",
            "version": "PDO 2.81.0",
            "prompts": "#### Design\nDraw it",
        },
        "screenshot": { "media_type": "image/png", "data": BASE64.encode(b"\x89PNG fake") },
    })
}

#[tokio::test]
async fn each_actor_sees_the_destinations_offered_to_it_under_the_extension_s_name() {
    let (daemon, _control) = daemon().await;
    let url = daemon.url();
    assert_eq!(
        destinations(&url, "alice").await,
        json!({
            "destinations": [
                {
                    "id": "test/inbox", "label": "Test inbox", "accepts_prompts": true,
                    "icon": "/ext/test/icon.svg", "extension": "test",
                    "extension_name": "Test extension",
                },
                {
                    "id": "test/board", "label": "Test board", "accepts_prompts": false,
                    "icon": null, "extension": "test", "extension_name": "Test extension",
                },
            ],
            "last": null,
        })
    );
    let guest = destinations(&url, "guest").await;
    let ids: Vec<&str> = guest["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["test/board"]);
}

#[tokio::test]
async fn a_report_reaches_the_extension_whole_and_becomes_the_last_choice() {
    let (daemon, control) = daemon().await;
    let url = daemon.url();

    let (status, sent) = send(&url, "alice", report("test/inbox")).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(
        sent,
        json!({ "reference": "BUG-1", "link": "/ext/test/bug-reports/1" })
    );

    let received = control.bug_reports.received();
    assert_eq!(received.len(), 1);
    let got = &received[0].report;
    assert_eq!(got.destination, "inbox");
    assert_eq!(got.title, "The node stays running");
    assert!(got
        .text
        .starts_with("### Description\nThe node stays running"));
    assert_eq!(got.fields.version.as_deref(), Some("PDO 2.81.0"));
    assert_eq!(got.fields.prompts.as_deref(), Some("#### Design\nDraw it"));
    let shot = got.screenshot.as_ref().unwrap();
    assert_eq!(shot.media_type, "image/png");
    assert_eq!(shot.bytes, b"\x89PNG fake");

    // The extension serves what it received at the link it answered.
    let resp = client()
        .get(format!("{url}/ext/test/bug-reports/1"))
        .send()
        .await
        .unwrap();
    let item: Value = resp.json().await.unwrap();
    assert_eq!(item["actor"], "identified:test/alice");
    assert_eq!(item["screenshot"]["size"], 9);

    // Alice's last choice is the inbox; Bob has none.
    assert_eq!(destinations(&url, "alice").await["last"], "test/inbox");
    assert_eq!(destinations(&url, "bob").await["last"], Value::Null);
}

#[tokio::test]
async fn a_report_is_refused_before_it_leaves() {
    let (daemon, control) = daemon().await;
    let url = daemon.url();

    // The prompts, toward the board that refuses them.
    let (status, refused) = send(&url, "alice", report("test/board")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["error"], "bug_report_refused");
    assert_eq!(
        refused["message"],
        "Test board does not accept prompts: uncheck Include prompts"
    );

    // No description.
    let mut empty = report("test/inbox");
    empty["fields"]["description"] = json!("  ");
    let (status, refused) = send(&url, "alice", empty).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["message"], "describe the bug first");

    // A destination the guest is not offered, one nobody declares, the core's.
    let (status, refused) = send(&url, "guest", report("test/inbox")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refused["error"], "unknown_destination");
    let (status, _) = send(&url, "alice", report("nobody/inbox")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&url, "alice", report("github")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A screenshot that is no image, or no base64.
    let mut text = report("test/inbox");
    text["screenshot"]["media_type"] = json!("text/plain");
    let (status, _) = send(&url, "alice", text).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut garbled = report("test/inbox");
    garbled["screenshot"]["data"] = json!("%%%");
    let (status, _) = send(&url, "alice", garbled).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The extension's own refusal reaches the caller.
    control
        .bug_reports
        .refuse(Some("the inbox is full".to_string()));
    let (status, refused) = send(&url, "alice", report("test/inbox")).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        refused,
        json!({ "error": "bug_report_not_sent", "message": "the inbox is full" })
    );

    assert!(control.bug_reports.received().is_empty());
    assert_eq!(destinations(&url, "alice").await["last"], Value::Null);
}

#[tokio::test]
async fn the_core_s_destination_is_remembered_for_its_actor_alone() {
    let (daemon, _control) = daemon().await;
    let url = daemon.url();
    let (status, _) = remember(&url, "alice", "github").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(destinations(&url, "alice").await["last"], "github");
    assert_eq!(destinations(&url, "bob").await["last"], Value::Null);

    // An offered destination too; one not offered, never.
    let (status, _) = remember(&url, "alice", "test/board").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(destinations(&url, "alice").await["last"], "test/board");
    let (status, refused) = remember(&url, "guest", "test/inbox").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refused["error"], "unknown_destination");
}

#[tokio::test]
async fn without_extension_no_destination_is_offered() {
    let daemon = TestDaemon::spawn(|repo| flow::seed_solo(repo, &[("solo", false)]))
        .await
        .unwrap();
    let url = daemon.url();
    let resp = client()
        .get(format!("{url}/bug-report/destinations"))
        .send()
        .await
        .unwrap();
    let (status, body) = answer(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "destinations": [], "last": null }));
    let (status, _) = answer(
        client()
            .put(format!("{url}/bug-report/last"))
            .json(&json!({ "destination": "github" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn the_cli_lists_sends_and_remembers_as_the_window_does() {
    let (daemon, control) = daemon().await;

    let listed = pdo(&daemon, &["bug-report", "destinations"]).await;
    assert_eq!(
        listed.ok(),
        "test/inbox\tTest inbox\taccepts prompts\ntest/board\tTest board\n"
    );

    let shot = daemon.repo_root().join("shot.png");
    std::fs::write(&shot, b"\x89PNG cli").unwrap();
    let sent = pdo(
        &daemon,
        &[
            "bug-report",
            "send",
            "test/board",
            "--description",
            "It breaks from the CLI",
            "--screenshot",
            shot.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(
        sent.ok(),
        "Bug report sent: BUG-1 /ext/test/bug-reports/1\n"
    );
    let received = control.bug_reports.received();
    let got = &received[0].report;
    assert_eq!(got.destination, "board");
    assert_eq!(got.text, "### Description\nIt breaks from the CLI");
    assert_eq!(got.screenshot.as_ref().unwrap().bytes, b"\x89PNG cli");

    let json = pdo(&daemon, &["bug-report", "destinations", "--json"]).await;
    assert_eq!(json.json()["last"], "test/board");

    let unknown = pdo(
        &daemon,
        &["bug-report", "send", "nobody/x", "--description", "x"],
    )
    .await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);

    assert_eq!(
        pdo(&daemon, &["bug-report", "last", "github"]).await.code,
        Some(0)
    );
    let listed = pdo(&daemon, &["bug-report", "destinations"]).await;
    assert!(
        listed.ok().ends_with("Last used: github\n"),
        "{}",
        listed.stdout
    );
}
