//! The route coverage (ADR-0080, #914): **every route of the router is called
//! by at least one integration test of the daemon**, method by method
//! (`tests/route_coverage.rs`).
//!
//! How it reads both sides, statically — nextest runs one process per test, so no
//! runtime recorder could see the calls the other tests make:
//!
//! - **the routes** ([`router`]): the `.route("<path>", <method router>)` calls of
//!   every context router the daemon merges, each top-level `get(`/`post(`/`put(`/
//!   `patch(`/`delete(` of the method router being one route. When a router
//!   moves, point [`ROUTER_SOURCES`] at its new home.
//! - **the calls** ([`calls`]): every string literal of `crates/pdo/tests/**/*.rs`
//!   that reads as a URL path (`"/runs"`, `"{}/runs/{run_id}/refs"`,
//!   `"ws://{}/ws"`, …) is matched to the most specific route template, and its
//!   HTTP method is read from the call the literal is an argument of: `.post(…)`,
//!   `reqwest::get(…)`, `get_json(…)`, a `Method::PUT` argument, a local helper
//!   whose body issues a single method, or — for `let url = format!(…)` — the
//!   call `url` is later passed to. A `ws://` literal is a GET (the upgrade).
//!
//! A literal the reader cannot tie to a method counts for nothing: write the call
//! so it can (`client.post(format!(…))`), don't teach this module your helper.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod calls;
mod router;

pub use calls::calls_in;
pub use router::router_routes;

/// The files holding the assembled router (relative to the repository root),
/// and the function that builds each part: the router of every bounded context,
/// which the composition root merges (`crates/pdo/src/contexts/mod.rs`) in front
/// of the SPA.
pub const ROUTER_SOURCES: &[(&str, &str)] = &[
    (
        "crates/pdo-sandbox/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-workspace/src/adapters/http/mod.rs",
        "pub fn router<S: ProjectSettings>(",
    ),
    (
        "crates/pdo-platform/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-skill/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-session/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-pipeline/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    ("crates/pdo-run/src/adapters/http/mod.rs", "pub fn router("),
    (
        "crates/pdo-trigger/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-review/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-stats/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-settings/src/adapters/http/mod.rs",
        "pub fn router(",
    ),
    (
        "crates/pdo-telemetry/src/adapters/http.rs",
        "pub fn router(",
    ),
    // The core's side of the extensions (`GET /ext`); each extension's own
    // routes, nested under `/ext/<id>/`, are the extension's.
    ("crates/pdo/src/extensions.rs", "pub fn router("),
    // Who the caller is, and the display names (`/actors`).
    ("crates/pdo/src/actors.rs", "pub(crate) fn router("),
    // The question of authorization (`POST /authorize`, CORE-5).
    ("crates/pdo/src/authorize.rs", "pub fn router("),
];

const VERBS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// `(METHOD, path template)`.
pub type Route = (String, String);

/// The routes of every bounded context — every router [`ROUTER_SOURCES`]
/// lists.
pub fn context_routes() -> Vec<Route> {
    let root = crate::workspace_root();
    ROUTER_SOURCES
        .iter()
        .flat_map(|(file, marker)| {
            let src = std::fs::read_to_string(root.join(file))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
            router_routes(&src, marker)
        })
        .collect()
}

/// The daemon's integration tests (`crates/pdo/tests/**/*.rs`), as
/// `(path relative to tests/, source)`.
pub fn test_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let tests = crate::workspace_root().join("crates/pdo/tests");
    let mut files = Vec::new();
    walk(&tests, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(&tests).unwrap().display().to_string();
            (rel, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

/// The routes of `routes` no source of `sources` calls, sorted.
pub fn uncovered(routes: &[Route], sources: &[(String, String)]) -> Vec<Route> {
    let paths: BTreeSet<String> = routes.iter().map(|(_, p)| p.clone()).collect();
    let covered = calls_in(sources, &paths);
    let mut missing: Vec<Route> = routes
        .iter()
        .filter(|r| !covered.contains_key(*r))
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    missing
}
