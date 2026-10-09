//! CORE-63 over a real daemon, with the test extension: a review comment
//! keeps the Actor of the request that sent it, on the comment the API lists
//! and on the Run event the extensions receive with its text; a resolution and
//! a reopening carry the Actor who decided.

use std::time::Duration;

use serde_json::{json, Value};

use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension};

const PIPELINE_YAML: &str = r#"name: review-actor
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: solo
    name: solo
    type: agent
    isolated_worktree: false
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
    target: { node: solo, port: in }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    crate::common::flow::write_pipeline(repo, "review-actor", PIPELINE_YAML)?;
    crate::common::flow::git_init(repo)
}

async fn post(daemon: &TestDaemon, path: &str, body: Value, actor: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .header("x-pdo-test-actor", actor)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// The Run events of `kind` the test extension received, waited for.
async fn received(control: &Control, kind: &str, count: usize) -> Vec<Value> {
    let mut seen = Vec::new();
    for _ in 0..100 {
        seen = control
            .events()
            .into_iter()
            .filter(|e| e["type"] == "event" && e["event"]["kind"] == kind)
            .map(|e| e["event"]["payload"].clone())
            .collect();
        if seen.len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    seen
}

#[tokio::test]
async fn a_review_comment_and_its_events_carry_the_actor() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, vec![Box::new(ext)])
        .await
        .unwrap();
    let (status, run) = post(
        &daemon,
        "/runs",
        json!({ "pipeline": "review-actor", "input": "go", "target_repo": daemon.target_repo() }),
        "alice",
    )
    .await;
    assert!(status < 300, "{run}");
    let run_id = run["run_id"].as_str().unwrap().to_string();

    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/review/comments/send"),
        json!({ "comments": [{ "path": "lib.rs", "side": "new", "line": 1, "text": "Why `a`?" }] }),
        "alice",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let alice = json!({ "kind": "identified", "namespace": "test", "id": "alice" });
    assert_eq!(body["sent"][0]["actor"], alice);
    assert_eq!(body["sent"][0]["author"], "user");

    let listed = daemon
        .get_json(&format!("/runs/{run_id}/review/comments"))
        .await;
    assert_eq!(listed["comments"][0]["actor"], alice);

    let sent = received(&control, "review_comment_sent", 1).await;
    assert_eq!(sent.len(), 1, "{:?}", control.events());
    assert_eq!(sent[0]["actor"], alice);
    assert_eq!(sent[0]["text"], "Why `a`?");

    let path = format!("/runs/{run_id}/review/comments/rc-001");
    let (status, body) = post(&daemon, &format!("{path}/resolve"), json!({}), "bob").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["comment"]["resolved_by_actor"]["id"], "bob");
    let (status, _) = post(&daemon, &format!("{path}/reopen"), json!({}), "alice").await;
    assert_eq!(status, 200);
    assert_eq!(
        received(&control, "review_comment_resolved", 1).await[0]["actor"]["id"],
        "bob"
    );
    assert_eq!(
        received(&control, "review_comment_reopened", 1).await[0]["actor"]["id"],
        "alice"
    );
}
