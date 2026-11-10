//! Layer 3a — `pdo session` through the real binary, against a real daemon
//! with the test extension and a real tmux: the status bar's gauges, and a
//! Session hébergée read, configured, terminated and relaunched by the CLI;
//! `4` for a session the daemon does not know.

use serde_json::json;

use crate::cli_operator::pdo;
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension, ACTOR_HEADER};

/// A daemon with the test extension, whose agent sessions only sleep.
async fn daemon() -> TestDaemon {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    TestDaemon::spawn_configured(
        |_| Ok(()),
        move |root, config| {
            config.tmux_cmd_override = Some("exec sleep 600".to_string());
            config.sandbox_home_override = Some(root.join("home"));
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_hosted_session_is_read_configured_terminated_and_relaunched_by_the_cli() {
    let daemon = daemon().await;
    let folder = tempfile::tempdir().unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/ext/test/sessions", daemon.url()))
        .header(ACTOR_HEADER, "alice")
        .json(&json!({
            "key": "a1", "kind": "agent",
            "folder": { "kind": "path", "path": folder.path() },
            "config": { "prompt": "Bonjour" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let session: serde_json::Value = resp.json().await.unwrap();
    let id = session["id"].as_str().unwrap().to_string();

    let status = pdo(&daemon, &["session", "status"]).await;
    assert!(status.ok().starts_with("pdo "), "{}", status.stdout);
    assert!(pdo(&daemon, &["session", "status", "--json"]).await.json()["version"].is_string());

    let shown = pdo(&daemon, &["session", "show", &id]).await;
    assert!(
        shown
            .ok()
            .starts_with(&format!("{id}  agent a1 of test (running)\n")),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("prompt: Bonjour\n"),
        "{}",
        shown.stdout
    );

    // Configured: the fields given change, the others stay.
    let configured = pdo(
        &daemon,
        &["session", "configure", &id, "--model", "opus", "--json"],
    )
    .await
    .json();
    assert_eq!(configured["config"]["model"], "opus", "{configured}");
    assert_eq!(configured["config"]["prompt"], "Bonjour", "{configured}");

    let terminated = pdo(&daemon, &["session", "terminate", &id]).await;
    assert_eq!(terminated.ok(), format!("{id} terminated (ended)\n"));
    let relaunched = pdo(&daemon, &["session", "relaunch", &id, "--json"])
        .await
        .json();
    assert_eq!(relaunched["state"], "running", "{relaunched}");

    let events = pdo(&daemon, &["session", "events", &id]).await;
    let kinds: Vec<&str> = events
        .ok()
        .lines()
        .filter_map(|l| l.split('\t').nth(1))
        .collect();
    assert!(
        kinds.len() >= 3,
        "opened, terminated, relaunched: {}",
        events.stdout
    );
    assert!(pdo(&daemon, &["session", "events", &id, "--json"])
        .await
        .json()
        .is_array());

    pdo(&daemon, &["session", "terminate", &id]).await.ok();
    let unknown = pdo(&daemon, &["session", "show", "pdo-hosted-nope"]).await;
    assert_eq!(unknown.code, Some(4), "{}", unknown.stderr);
}
