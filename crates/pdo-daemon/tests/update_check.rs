//! Layer 3 — the daemon's version check against a fixture release source (#697).
//!
//! Drives a **real daemon** whose release source is a **local fixture server**, so
//! no test reaches GitHub, and whose home override puts `~/.pdo/update/check.json`
//! under the test's tempdir. Prior art: `cost_prices.rs`.
//!
//! What is proven here (ADR-0004 — an AC closes at layer ≥ 3):
//!   1. the routes are REGISTERED (anti-SPA gate: content-type, not just status);
//!   2. a newer fixture → `latest_version` + `checked_at` + `newer_available`, and the
//!      cache file is written;
//!   3. reads are cache-only: N `GET /update` cost ZERO fixture hits, a periodic pass
//!      within the interval costs zero too;
//!   4. unreachable source → `latest_version: null`, a reason, no blocking error; a
//!      manual check answers 502 with the refreshed date and the error;
//!   5. `update_check=false` via `PUT /settings` → zero hits on a periodic pass, UI
//!      value `null`, `POST /update/check` refused 409, setting persisted in `/settings`;
//!   6. « Check now » forces a hit and moves `checked_at`;
//!   7. the install-method / supervision fields are one of the declared values with
//!      the manual command that matches.

use crate::common::TestDaemon;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The shape of GitHub's `releases/latest`, trimmed to what the parser reads.
const RELEASE_NEWER: &str = r#"{"tag_name":"v99.0.0","name":"pdo 99.0.0","prerelease":false}"#;

struct Fixture {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    _task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn url(&self) -> String {
        format!("http://{}/releases/latest", self.addr)
    }
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

async fn spawn_fixture(body: &'static str) -> Fixture {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/releases/latest",
        axum::routing::get(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                ([("content-type", "application/json")], body)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Fixture {
        addr,
        hits,
        _task: task,
    }
}

/// An address nothing listens on: bind, read the port, drop the listener.
async fn dead_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/releases/latest")
}

fn seed(_repo: &Path) -> anyhow::Result<()> {
    Ok(())
}

async fn get_update(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::get(format!("{}/update", daemon.url()))
        .await
        .unwrap()
}

async fn update_json(daemon: &TestDaemon) -> serde_json::Value {
    get_update(daemon).await.json().await.unwrap()
}

async fn check_now(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/update/check", daemon.url()))
        .send()
        .await
        .unwrap()
}

async fn put_update_check(daemon: &TestDaemon, on: bool) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "update_check": on }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json().await.unwrap()
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

fn cache_path(daemon: &TestDaemon) -> std::path::PathBuf {
    daemon.repo_root().join(".pdo/update/check.json")
}

#[tokio::test]
async fn routes_are_registered_and_a_newer_release_is_reported() {
    let fixture = spawn_fixture(RELEASE_NEWER).await;
    let daemon = TestDaemon::spawn_with_update_source(seed, fixture.url())
        .await
        .unwrap();

    // Before any check: cache-only read, honest "not checked yet", zero egress.
    let resp = get_update(&daemon).await;
    assert_eq!(resp.status(), 200);
    assert!(
        content_type(&resp).starts_with("application/json"),
        "GET /update must be registered (the SPA fallback answers text/html)"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["installed_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["latest_version"], serde_json::Value::Null);
    assert_eq!(body["newer_available"], false);
    assert_eq!(body["check_enabled"], true);
    assert_eq!(body["reason"], "Not checked yet.");
    assert_eq!(fixture.hits(), 0, "a page load must never hit the source");

    // The boot/periodic pass (driven deterministically) checks once.
    daemon.run_update_check_tick().await;
    assert_eq!(fixture.hits(), 1);
    let body = update_json(&daemon).await;
    assert_eq!(body["latest_version"], "99.0.0");
    assert_eq!(body["newer_available"], true, "the badge's flag");
    assert!(body["checked_at"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(body["reason"], serde_json::Value::Null);
    assert_eq!(body["last_error"], serde_json::Value::Null);
    assert_eq!(body["source_url"], fixture.url());

    // Cache on disk, with its date.
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cache_path(&daemon)).unwrap()).unwrap();
    assert_eq!(doc["schema"], "update-check-v1");
    assert_eq!(doc["latest_version"], "99.0.0");
    assert_eq!(doc["checked_at"], body["checked_at"]);

    // Cache: several page loads and another periodic pass within the interval →
    // still ONE request.
    for _ in 0..3 {
        update_json(&daemon).await;
    }
    daemon.run_update_check_tick().await;
    assert_eq!(
        fixture.hits(),
        1,
        "reads and an in-interval pass are cache-only"
    );

    // Install method / supervision: declared values, matching command.
    let method = body["install_method"].as_str().unwrap();
    assert!(["homebrew", "script", "unknown"].contains(&method));
    let cmd = body["manual_command"].as_str().unwrap();
    match method {
        "homebrew" => assert_eq!(cmd, "brew update && brew upgrade Loulen/tap/pdo"),
        "script" => assert!(cmd.contains("pdo-installer.sh")),
        _ => assert!(cmd.starts_with("Build from source")),
    }
    let sup = body["supervision"].as_str().unwrap();
    assert!(["systemd", "launchd", "none"].contains(&sup));
}

#[tokio::test]
async fn check_now_forces_a_hit_and_moves_the_date() {
    let fixture = spawn_fixture(RELEASE_NEWER).await;
    let daemon = TestDaemon::spawn_with_update_source(seed, fixture.url())
        .await
        .unwrap();
    daemon.run_update_check_tick().await;
    let first = update_json(&daemon).await["checked_at"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let resp = check_now(&daemon).await;
    assert_eq!(resp.status(), 200);
    assert!(content_type(&resp).starts_with("application/json"));
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(fixture.hits(), 2, "Check now bypasses the interval");
    assert_eq!(body["latest_version"], "99.0.0");
    assert_ne!(
        body["checked_at"].as_str().unwrap(),
        first,
        "the date must move"
    );
}

#[tokio::test]
async fn unreachable_source_is_null_with_a_reason_and_never_blocks() {
    let daemon = TestDaemon::spawn_with_update_source(seed, dead_url().await)
        .await
        .unwrap();
    daemon.run_update_check_tick().await;

    let resp = get_update(&daemon).await;
    assert_eq!(
        resp.status(),
        200,
        "the read path does not depend on the source"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["latest_version"], serde_json::Value::Null);
    assert_eq!(body["newer_available"], false);
    assert_eq!(body["reason"], "Release source unreachable at last check.");
    assert!(
        body["checked_at"].as_str().is_some(),
        "a failed check is still a check"
    );
    assert!(body["last_error"].as_str().unwrap().contains("unreachable"));

    // Manual check: 502 carrying the refreshed state, no panic, values kept.
    let resp = check_now(&daemon).await;
    assert_eq!(resp.status(), 502);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("unreachable"));
    assert_eq!(body["latest_version"], serde_json::Value::Null);
    assert!(body["checked_at"].as_str().is_some());
}

#[tokio::test]
async fn disabling_the_check_stops_all_egress_and_persists() {
    let fixture = spawn_fixture(RELEASE_NEWER).await;
    let daemon = TestDaemon::spawn_with_update_source(seed, fixture.url())
        .await
        .unwrap();

    // Default: ON, disclosed as such.
    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(settings["update_check"]["effective"], true);
    assert_eq!(settings["update_check"]["default"], true);

    // Turn it off: persisted, and the periodic pass makes NO request.
    let view = put_update_check(&daemon, false).await;
    assert_eq!(view["update_check"]["effective"], false);
    assert_eq!(view["update_check"]["source"], "stored");
    assert_eq!(view["update_check"]["stored"], false);
    daemon.run_update_check_tick().await;
    assert_eq!(fixture.hits(), 0, "off: no request ever leaves the daemon");

    let body = update_json(&daemon).await;
    assert_eq!(body["check_enabled"], false);
    assert_eq!(body["latest_version"], serde_json::Value::Null);
    assert_eq!(body["newer_available"], false);
    assert_eq!(body["reason"], "Update check is off.");

    // Check now is refused while off.
    let resp = check_now(&daemon).await;
    assert_eq!(resp.status(), 409);
    assert_eq!(fixture.hits(), 0);

    // Persisted: a fresh GET /settings still says off.
    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(settings["update_check"]["effective"], false);

    // Re-enable: one check runs on its own; then the value and the flag are back.
    put_update_check(&daemon, true).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let body = update_json(&daemon).await;
        if body["latest_version"] == "99.0.0" {
            assert_eq!(body["newer_available"], true);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "re-enable must trigger a check: {body}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(fixture.hits(), 1);

    // A stale cache from before the switch is hidden while off, kept underneath.
    put_update_check(&daemon, false).await;
    assert_eq!(
        update_json(&daemon).await["latest_version"],
        serde_json::Value::Null
    );
    put_update_check(&daemon, true).await;
    assert_eq!(update_json(&daemon).await["latest_version"], "99.0.0");
}
