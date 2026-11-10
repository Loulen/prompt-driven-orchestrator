//! The whole `/ext/` prefix belongs to the API (#22): against the real daemon,
//! an extension that is not there answers `404` JSON `extension_not_found`,
//! never the SPA in `200`, whatever the method — with no extension at all and
//! with the internal test extension loaded. The SPA's own deep links keep
//! their fallback.

use reqwest::Method;
use serde_json::{json, Value};

use crate::common::flow::seed_solo;
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension};

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

/// `method path`: `(status, content-type, body)`, the body `Null` when not JSON.
async fn call(daemon: &TestDaemon, method: Method, path: &str) -> (u16, String, Value) {
    let resp = reqwest::Client::new()
        .request(method, format!("{}{path}", daemon.url()))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        content_type,
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}

/// Asserts `method path` answers `404` JSON with the `slug`, naming `id`.
async fn assert_404(daemon: &TestDaemon, method: Method, path: &str, slug: &str, id: &str) {
    let (status, content_type, body) = call(daemon, method.clone(), path).await;
    assert_eq!(status, 404, "{method} {path}: {body}");
    assert_eq!(content_type, "application/json", "{method} {path}");
    assert_eq!(body["error"], slug, "{method} {path}: {body}");
    assert_eq!(body["id"], id, "{method} {path}: {body}");
    assert!(
        body["message"].as_str().unwrap().contains(id),
        "{method} {path}: {body}"
    );
}

/// Asserts `GET path` answers the SPA: `200` HTML.
async fn assert_spa(daemon: &TestDaemon, path: &str) {
    let (status, content_type, _) = call(daemon, Method::GET, path).await;
    assert_eq!(status, 200, "GET {path}");
    assert!(
        content_type.starts_with("text/html"),
        "GET {path}: {content_type}"
    );
}

#[tokio::test]
async fn without_extension_every_ext_id_answers_404_json_and_the_spa_keeps_its_links() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    for path in ["/ext/hello", "/ext/hello/api/x"] {
        assert_404(&daemon, Method::GET, path, "extension_not_found", "hello").await;
    }
    assert_404(
        &daemon,
        Method::POST,
        "/ext/hello/x",
        "extension_not_found",
        "hello",
    )
    .await;
    assert_404(
        &daemon,
        Method::DELETE,
        "/ext/hello",
        "extension_not_found",
        "hello",
    )
    .await;
    assert_eq!(daemon.get_json("/ext").await, json!([]));

    // Non-regression of the SPA fallback: its root and a deep link.
    assert_spa(&daemon, "/").await;
    assert_spa(&daemon, "/runs/some-run/review").await;
}

#[tokio::test]
async fn with_the_test_extension_its_routes_answer_and_another_id_is_not_found() {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    assert_eq!(daemon.get_json("/ext/test").await["id"], "test");
    let (status, content_type, _) = call(&daemon, Method::GET, "/ext/test/ui.js").await;
    assert_eq!(status, 200);
    assert!(
        content_type.starts_with("text/javascript"),
        "{content_type}"
    );

    assert_404(&daemon, Method::GET, "/ext/test/nope", "not_found", "test").await;
    assert_404(
        &daemon,
        Method::GET,
        "/ext/other",
        "extension_not_found",
        "other",
    )
    .await;
    // An id that only starts like the loaded one is another id.
    assert_404(
        &daemon,
        Method::GET,
        "/ext/tester/ui.js",
        "extension_not_found",
        "tester",
    )
    .await;
    assert_eq!(daemon.get_json("/ext").await[0]["id"], "test");
    assert_spa(&daemon, "/runs/some-run/review").await;
}
