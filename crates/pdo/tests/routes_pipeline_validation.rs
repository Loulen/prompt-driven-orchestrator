//! Layer 4 (ADR-0080) — the validation of a pipeline (CONTEXT.md « Pipeline
//! validé ») through the real daemon: the list exposes it, `PUT
//! /pipelines/{id}/validation` replaces it and announces it on `/ws`, a
//! pipeline is born a test pipeline, a rename carries its validation, a
//! deletion forgets it, an export leaves it behind, and a deleted Projet is
//! withdrawn from it.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::common::{flow, ws_text, TestDaemon};

const LEGACY: &str = "legacy";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[(LEGACY, false)])
}

async fn send(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{}{path}", daemon.url()));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn put_validation(daemon: &TestDaemon, id: &str, validation: Value) -> (u16, Value) {
    send(
        daemon,
        reqwest::Method::PUT,
        &format!("/pipelines/{id}/validation"),
        Some(validation),
    )
    .await
}

async fn listed_validation(daemon: &TestDaemon, id: &str) -> Value {
    let list = daemon.get_json("/pipelines").await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("pipeline {id} not listed: {list}"))["validation"]
        .clone()
}

async fn create_project(daemon: &TestDaemon, name: &str) -> String {
    let (status, body) = flow::post_json(daemon, "/projects", json!({ "name": name })).await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn an_existing_pipeline_is_validated_for_every_projet_and_a_new_one_is_a_test_pipeline() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let none = json!({ "kind": "none" });
    assert_eq!(
        listed_validation(&daemon, LEGACY).await,
        json!({ "kind": "all" })
    );

    let (status, created) =
        flow::post_json(&daemon, "/pipelines", json!({ "name": "Fresh" })).await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(listed_validation(&daemon, "fresh").await, none);

    let (status, copy) = send(
        &daemon,
        reqwest::Method::POST,
        &format!("/pipelines/{LEGACY}/duplicate"),
        None,
    )
    .await;
    assert_eq!(status, 201, "{copy}");
    assert_eq!(
        listed_validation(&daemon, copy["id"].as_str().unwrap()).await,
        none
    );
    assert_eq!(
        listed_validation(&daemon, LEGACY).await,
        json!({ "kind": "all" })
    );

    let document = reqwest::get(format!("{}/pipelines/{LEGACY}/document", daemon.url()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let (status, imported) = flow::post_json(
        &daemon,
        "/pipelines/import",
        json!({ "document": document }),
    )
    .await;
    assert_eq!(status, 201, "{imported}");
    assert_eq!(
        listed_validation(&daemon, imported["id"].as_str().unwrap()).await,
        none
    );
}

#[tokio::test]
async fn a_validation_is_replaced_announced_and_never_exported() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let alpha = create_project(&daemon, "Alpha").await;
    let bravo = create_project(&daemon, "Bravo").await;
    let mut ws = daemon.connect_ws().await.unwrap();

    let both = json!({ "kind": "projects", "project_ids": [alpha, bravo] });
    let (status, body) = put_validation(&daemon, LEGACY, both.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["validation"], both);
    assert_eq!(listed_validation(&daemon, LEGACY).await, both);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut announced = None;
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, ws.next()).await {
        let Some(text) = ws_text(&message) else {
            continue;
        };
        let message: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        if message["type"] == "pipeline_validation_changed" {
            announced = Some(message);
            break;
        }
    }
    assert_eq!(
        announced,
        Some(json!({ "type": "pipeline_validation_changed", "pipeline_id": LEGACY }))
    );

    let (status, body) = put_validation(
        &daemon,
        LEGACY,
        json!({ "kind": "projects", "project_ids": [] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["validation"],
        json!({ "kind": "none" }),
        "nothing ticked"
    );

    let document = reqwest::get(format!("{}/pipelines/{LEGACY}/document", daemon.url()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !document.contains(&alpha),
        "an export carries no Projet: {document}"
    );
    assert!(!document.contains("validation"), "{document}");
}

#[tokio::test]
async fn an_unknown_pipeline_or_projet_is_refused_before_any_write() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let alpha = create_project(&daemon, "Alpha").await;

    let (status, _) = put_validation(&daemon, "ghost", json!({ "kind": "none" })).await;
    assert_eq!(status, 404);

    let (status, body) = put_validation(
        &daemon,
        LEGACY,
        json!({ "kind": "projects", "project_ids": [alpha, "prj-nope"] }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "no such project: prj-nope");
    assert_eq!(
        listed_validation(&daemon, LEGACY).await,
        json!({ "kind": "all" })
    );
}

#[tokio::test]
async fn a_rename_carries_the_validation_and_a_deletion_forgets_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let alpha = create_project(&daemon, "Alpha").await;
    let only_alpha = json!({ "kind": "projects", "project_ids": [alpha] });
    put_validation(&daemon, LEGACY, only_alpha.clone()).await;

    let (status, body) = send(
        &daemon,
        reqwest::Method::PUT,
        &format!("/pipelines/{LEGACY}/rename"),
        Some(json!({ "name": "Renamed" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(listed_validation(&daemon, "renamed").await, only_alpha);

    let (status, body) = send(&daemon, reqwest::Method::DELETE, "/pipelines/renamed", None).await;
    assert_eq!(status, 200, "{body}");
    // A pipeline created again under the same id starts over: a test pipeline,
    // not the deleted one's Projets.
    flow::post_json(&daemon, "/pipelines", json!({ "name": "Renamed" })).await;
    assert_eq!(
        listed_validation(&daemon, "renamed").await,
        json!({ "kind": "none" })
    );
}

#[tokio::test]
async fn a_deleted_projet_is_withdrawn_and_a_pipeline_left_with_none_is_a_test_pipeline() {
    let daemon =
        TestDaemon::spawn(|repo| flow::seed_solo(repo, &[("shared", false), ("lone", false)]))
            .await
            .unwrap();
    let alpha = create_project(&daemon, "Alpha").await;
    let bravo = create_project(&daemon, "Bravo").await;
    put_validation(
        &daemon,
        "shared",
        json!({ "kind": "projects", "project_ids": [alpha, bravo] }),
    )
    .await;
    put_validation(
        &daemon,
        "lone",
        json!({ "kind": "projects", "project_ids": [alpha] }),
    )
    .await;

    let (status, body) = send(
        &daemon,
        reqwest::Method::DELETE,
        &format!("/projects/{alpha}"),
        None,
    )
    .await;
    assert!(status == 200 || status == 204, "{status} {body}");

    assert_eq!(
        listed_validation(&daemon, "shared").await,
        json!({ "kind": "projects", "project_ids": [bravo] })
    );
    assert_eq!(
        listed_validation(&daemon, "lone").await,
        json!({ "kind": "none" })
    );
}

#[tokio::test]
async fn an_extension_refusing_pipeline_validate_refuses_the_validation_only() {
    use crate::test_extension::{Refusal, TestExtension};
    use pdo::extension_api::{ResourceKind, Verb};

    let (ext, _control) = TestExtension::new(Refusal::Only(
        ResourceKind::Pipeline,
        Verb::Action("validate".into()),
    ));
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();

    let (status, body) = put_validation(&daemon, LEGACY, json!({ "kind": "none" })).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(
        listed_validation(&daemon, LEGACY).await,
        json!({ "kind": "all" })
    );
    // Its other verbs stay allowed.
    let (status, body) = flow::post_json(&daemon, "/pipelines", json!({ "name": "Other" })).await;
    assert_eq!(status, 201, "{body}");
}
