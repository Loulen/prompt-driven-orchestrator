//! Layer 4 (ADR-0080) — `DELETE /triggers/{id}`, the one Trigger method no
//! integration test called before the refactor net (#914).

use crate::common::flow;
use crate::common::{ws_text, TestDaemon};
use futures_util::StreamExt;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

#[tokio::test]
async fn deleting_a_trigger_removes_it_announces_it_and_audits_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, trigger) = flow::post_json(
        &daemon,
        "/triggers",
        serde_json::json!({
            "name": "nightly",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(status, 201, "{trigger}");
    let id = trigger["id"].as_str().unwrap().to_string();
    let mut ws = daemon.connect_ws().await.unwrap();

    let resp = reqwest::Client::new()
        .delete(format!("{}/triggers/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    let resp = reqwest::get(format!("{}/triggers/{id}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let list = daemon.get_json("/triggers").await;
    assert!(
        !list.to_string().contains(&id),
        "gone from the list: {list}"
    );

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut announced = false;
    while !announced && tokio::time::Instant::now() < deadline {
        let next = tokio::time::timeout(std::time::Duration::from_millis(500), ws.next()).await;
        let Ok(Some(Ok(msg))) = next else { continue };
        if let Some(text) = ws_text(&msg) {
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            announced = value["type"] == "trigger_deleted" && value["trigger_id"] == id.as_str();
        }
    }
    assert!(announced, "the WebSocket carried trigger_deleted");

    let audit = daemon.get_json("/audit").await;
    assert!(
        audit.to_string().contains("trigger.deleted"),
        "the deletion is audited: {audit}"
    );

    let resp = reqwest::Client::new()
        .delete(format!("{}/triggers/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "trigger not found");
}
