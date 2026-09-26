//! Layer 4 (ADR-0080) — `POST /runs/{id}/review/comments/{cid}/reply`, the one
//! review route no integration test called before the refactor net (#914).
//!
//! A reply's author is deduced from the calling session; `resolved: true` is a
//! proposal unless the instance lets agents resolve (ADR-0067 §4).

use crate::common::flow;
use crate::common::TestDaemon;

const SOLO: &str = "solo";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[(SOLO, false)])
}

/// A live Run with one commit on its branch and one sent comment, `rc-001`.
async fn run_with_a_comment(daemon: &TestDaemon) -> String {
    let run_id = flow::create_run(daemon, SOLO).await;
    flow::wait_node_status(daemon, &run_id, "worker", "running").await;
    let wt = flow::worktree(daemon, &run_id);
    std::fs::write(
        wt.join("lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    flow::git(&wt, &["add", "lib.rs"]);
    flow::git(&wt, &["commit", "-q", "-m", "run work"]);
    let (status, body) = flow::post_json(
        daemon,
        &format!("/runs/{run_id}/review/comments/send"),
        serde_json::json!({
            "comments": [{ "path": "lib.rs", "side": "new", "line": 4, "text": "Name it delta." }]
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sent"][0]["id"], "rc-001");
    run_id
}

async fn reply(
    daemon: &TestDaemon,
    run_id: &str,
    comment_id: &str,
    node: Option<&str>,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let mut req = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/review/comments/{comment_id}/reply",
            daemon.url()
        ))
        .json(&body);
    if let Some(node) = node {
        req = req
            .header("X-PDO-Session-Run-Id", run_id)
            .header("X-PDO-Session-Node-Id", node);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn a_reply_is_threaded_under_the_comment_and_signed_by_the_calling_session() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = run_with_a_comment(&daemon).await;

    let (status, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        Some("worker"),
        serde_json::json!({ "text": "Renamed in the next delivery." }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["author"], "worker");
    assert_eq!(body["outcome"], "replied");
    assert_eq!(body["comment"]["id"], "rc-001");
    let replies = body["comment"]["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 1, "{body}");
    assert_eq!(replies[0]["text"], "Renamed in the next delivery.");

    // No session: the declared author, else the neutral `agent`.
    let (status, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        None,
        serde_json::json!({ "text": "Second thought.", "author": "reviewer-bot" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["author"], "reviewer-bot");
    let (_, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        None,
        serde_json::json!({ "text": "Third." }),
    )
    .await;
    assert_eq!(body["author"], "agent");

    let list = daemon
        .get_json(&format!("/runs/{run_id}/review/comments"))
        .await;
    assert_eq!(
        list["comments"][0]["replies"].as_array().unwrap().len(),
        3,
        "{list}"
    );
    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    let replied = events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "review_comment_replied")
        .count();
    assert_eq!(replied, 3);
}

#[tokio::test]
async fn resolved_is_a_proposal_by_default_and_a_resolution_when_agents_may_resolve() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = run_with_a_comment(&daemon).await;

    let (status, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        Some("worker"),
        serde_json::json!({ "text": "Done.", "resolved": true }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["outcome"], "proposed");
    assert_ne!(body["comment"]["status"], "resolved", "{body}");

    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "review_agent_can_resolve": true }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let (status, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        Some("worker"),
        serde_json::json!({ "text": "Really done.", "resolved": true }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["outcome"], "resolved");
    assert_eq!(body["comment"]["status"], "resolved", "{body}");

    // On a resolved comment, `resolved` is just a reply.
    let (_, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        Some("worker"),
        serde_json::json!({ "text": "Still here.", "resolved": true }),
    )
    .await;
    assert_eq!(body["outcome"], "replied");
}

#[tokio::test]
async fn reply_refuses_an_empty_text_an_unknown_comment_and_an_unknown_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = run_with_a_comment(&daemon).await;

    let (status, body) = reply(
        &daemon,
        &run_id,
        "rc-001",
        None,
        serde_json::json!({ "text": "   " }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "empty reply text");

    let (status, _) = reply(
        &daemon,
        &run_id,
        "rc-999",
        None,
        serde_json::json!({ "text": "hello" }),
    )
    .await;
    assert_eq!(status, 404);

    let (status, _) = reply(
        &daemon,
        "no-such-run",
        "rc-001",
        None,
        serde_json::json!({ "text": "hello" }),
    )
    .await;
    assert_eq!(status, 404);
}
