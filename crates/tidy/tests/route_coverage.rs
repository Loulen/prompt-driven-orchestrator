//! Structure test (ADR-0080, #914) — **every route of the router is called by at
//! least one integration test of the daemon**, method by method.
//!
//! Adding a route (or a method on an existing path) without a test against the
//! real daemon turns this red. How the router and the tests are read:
//! `src/routes/mod.rs`.

use std::collections::BTreeSet;

use tidy::routes::{calls_in, context_routes, router_routes, test_sources, uncovered, Route};

#[test]
fn every_route_is_called_by_at_least_one_integration_test() {
    let routes = context_routes();
    let missing = uncovered(&routes, &test_sources());
    assert!(
        missing.is_empty(),
        "{} route(s) of the router are called by no integration test (tests/**/*.rs) — \
         add a test against the real daemon (TestDaemon + reqwest) for each:\n{}",
        missing.len(),
        missing
            .iter()
            .map(|(m, p)| format!("  {m} {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_router_reader_sees_every_route_declaration() {
    let routes = context_routes();
    let paths: BTreeSet<&str> = routes.iter().map(|(_, p)| p.as_str()).collect();
    // Spot checks on the shapes the reader must handle.
    for expected in [
        ("GET", "/ws"),
        ("PUT", "/pipelines/{pipeline_id}"),
        ("DELETE", "/pipelines/{pipeline_id}"),
        ("POST", "/runs"),
        ("PATCH", "/runs/{run_id}/repos"),
        ("GET", "/pages/{name}/{*path}"),
        ("DELETE", "/stats/absorptions/member"),
    ] {
        assert!(
            routes.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?} missing from {routes:?}"
        );
    }
    assert!(paths.len() > 100, "{} paths only", paths.len());
}

/// The demonstration the issue asks for, kept as a test: a fictitious route, and
/// a fictitious method on a covered path, both come out uncovered.
#[test]
fn a_route_without_a_test_turns_the_coverage_red() {
    let src = std::fs::read_to_string(
        tidy::workspace_root().join("crates/pdo-settings/src/adapters/http/mod.rs"),
    )
    .unwrap();
    let injected = src.replacen(
        "pub fn router(settings: Arc<Settings>) -> Router {\n    Router::new()\n",
        "pub fn router(settings: Arc<Settings>) -> Router {\n    Router::new()\n        \
         .route(\"/net-914/fictitious\", get(fictitious))\n        \
         .route(\"/runs/reapable\", axum::routing::delete(fictitious))\n",
        1,
    );
    assert_ne!(injected, src, "the injection point moved");
    let routes = router_routes(&injected, "pub fn router(");
    assert_eq!(
        uncovered(&routes, &test_sources()),
        vec![
            ("DELETE".to_string(), "/runs/reapable".to_string()),
            ("GET".to_string(), "/net-914/fictitious".to_string()),
        ]
    );
}

#[test]
fn the_call_reader_ties_literals_to_their_method() {
    let routes: Vec<Route> = [
        ("GET", "/runs"),
        ("POST", "/runs"),
        ("GET", "/runs/{run_id}"),
        ("GET", "/runs/reapable"),
        ("DELETE", "/runs/{run_id}"),
        ("PUT", "/settings"),
        ("POST", "/runs/{run_id}/nodes/{node_id}/skip"),
        ("GET", "/ws"),
    ]
    .iter()
    .map(|(m, p)| (m.to_string(), p.to_string()))
    .collect();
    let paths: BTreeSet<String> = routes.iter().map(|(_, p)| p.clone()).collect();
    let src = r#####"
async fn raw_post(d: &D, path: &str) {}
async fn transition(d: &D, path: &str) { raw_post(d, path).await; }
async fn t(daemon: &D) {
    client.post(format!("{}/runs", daemon.url())).send();
    let x = daemon.get_json(&format!("/runs/{run_id}")).await;
    let url = format!("{}/runs/{}", daemon.url(), id);
    client.delete(&url).send();
    send(&daemon, reqwest::Method::PUT, "/settings", None).await;
    transition(&daemon, &format!("/runs/{run_id}/nodes/a/skip")).await;
    assert!(text.contains("/runs/reapable"));
    let ws = format!("ws://{}/ws", daemon.addr);
}
"#####;
    let covered = calls_in(&[("t.rs".to_string(), src.to_string())], &paths);
    let got: BTreeSet<&Route> = covered.keys().collect();
    let want: BTreeSet<Route> = [
        ("POST", "/runs"),
        ("GET", "/runs/{run_id}"),
        ("DELETE", "/runs/{run_id}"),
        ("PUT", "/settings"),
        ("POST", "/runs/{run_id}/nodes/{node_id}/skip"),
        ("GET", "/ws"),
    ]
    .iter()
    .map(|(m, p)| (m.to_string(), p.to_string()))
    .collect();
    assert_eq!(got, want.iter().collect(), "an assert on a text is no call");
}
