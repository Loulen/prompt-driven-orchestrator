//! Layer 3a smoke test — proves the TestDaemon helper boots a real daemon and
//! that an HTTP client can reach it. Future bug-driven tests (#17, #18, etc.)
//! build on this same harness.

mod common;

use common::TestDaemon;
use futures_util::StreamExt;

#[tokio::test]
async fn runs_endpoint_returns_empty_array_on_fresh_daemon() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();

    let resp = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let runs: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(runs.is_empty(), "fresh daemon should report no runs");
}

#[tokio::test]
async fn ws_emits_ready_on_connect() {
    let daemon = TestDaemon::spawn(|_repo_root| Ok(())).await.unwrap();

    let mut ws = daemon.connect_ws().await.unwrap();
    let msg = ws
        .next()
        .await
        .expect("ws closed before ready")
        .expect("ws error");
    let text = msg.into_text().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "ready");
}
