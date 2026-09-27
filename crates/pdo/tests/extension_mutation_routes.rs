//! Stage 4 (ADR-0080) — **every mutation route of an extracted context goes
//! through the Autorisation d'opération** (#930).
//!
//! The routes are read from the routers themselves (`route_coverage.rs`'s
//! reader, over the context routers it lists), so a context extracted later
//! inherits the check: its mutation routes (`POST`, `PUT`, `PATCH`, `DELETE`)
//! join the enumeration, and each needs a request below. With an extension that
//! refuses everything, every one of them answers `403 operation_refused`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::common::flow::git_init;
use crate::common::TestDaemon;
use crate::route_coverage::{context_routes, Route};
use crate::test_extension::{Refusal, TestExtension};

const MUTATIONS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// The `POST`s that mutate nothing, with why.
const NOT_MUTATIONS: &[(&str, &str, &str)] = &[(
    "POST",
    "/repos/provisioning/preview",
    "computes the plan of a worktree provisioning; stores nothing",
)];

/// One request aimed at a route: its method, its template, the concrete path
/// and the body that gets it past extraction.
struct Call {
    method: &'static str,
    template: &'static str,
    path: String,
    body: Option<Value>,
}

fn call(method: &'static str, template: &'static str, path: String, body: Option<Value>) -> Call {
    Call {
        method,
        template,
        path,
        body,
    }
}

/// A valid request per mutation route, over fixtures `seed` created while the
/// extension still allowed everything.
fn calls(repo: &Path, fixtures: &Fixtures) -> Vec<Call> {
    let repo = repo.to_string_lossy().to_string();
    let q = format!("path={repo}");
    let project = &fixtures.project;
    let site = fixtures.site.to_string_lossy().to_string();
    vec![
        call(
            "POST",
            "/pages",
            "/pages".into(),
            Some(json!({ "name": "other", "directory": site })),
        ),
        call("DELETE", "/pages/{name}", "/pages/site".into(), None),
        call("POST", "/update/check", "/update/check".into(), None),
        call("POST", "/update/apply", "/update/apply".into(), None),
        call("POST", "/repos/fetch", format!("/repos/fetch?{q}"), None),
        call(
            "POST",
            "/repos/fast-forward",
            format!("/repos/fast-forward?{q}"),
            Some(json!({ "branch": "main" })),
        ),
        call(
            "POST",
            "/repos/create",
            "/repos/create".into(),
            Some(json!({ "parent": fixtures.scratch, "name": "fresh" })),
        ),
        call(
            "PUT",
            "/settings/provisioning",
            "/settings/provisioning".into(),
            Some(json!({})),
        ),
        call(
            "POST",
            "/projects",
            "/projects".into(),
            Some(json!({ "name": "another" })),
        ),
        call(
            "PATCH",
            "/projects/{project_id}",
            format!("/projects/{project}"),
            Some(json!({ "name": "renamed" })),
        ),
        call(
            "DELETE",
            "/projects/{project_id}",
            format!("/projects/{project}"),
            None,
        ),
        call(
            "POST",
            "/projects/{project_id}/members",
            format!("/projects/{project}/members"),
            Some(json!({ "path": fixtures.scratch })),
        ),
        call(
            "DELETE",
            "/projects/{project_id}/members",
            format!("/projects/{project}/members"),
            Some(json!({ "path": repo })),
        ),
        call(
            "PUT",
            "/projects/{project_id}/provisioning",
            format!("/projects/{project}/provisioning"),
            Some(json!({})),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/wait-user",
            format!("/runs/{RUN}/nodes/worker/wait-user"),
            Some(json!({ "iter": 1 })),
        ),
        call(
            "POST",
            "/sessions/{session_id}/attach",
            format!("/sessions/pdo-{RUN}-worker/attach"),
            None,
        ),
        call(
            "POST",
            "/sessions/{run_id}/manager/attach",
            format!("/sessions/{RUN}/manager/attach"),
            None,
        ),
        call(
            "POST",
            "/sessions/{run_id}/shell",
            format!("/sessions/{RUN}/shell"),
            None,
        ),
        call(
            "POST",
            "/sessions/libassist",
            "/sessions/libassist".into(),
            None,
        ),
        call(
            "DELETE",
            "/sessions/libassist",
            "/sessions/libassist".into(),
            None,
        ),
        call(
            "PUT",
            "/sessions/libassist/focus",
            "/sessions/libassist/focus".into(),
            Some(json!({ "pipeline_id": "other" })),
        ),
        // Saves the focused template: `seed` declared the focus.
        call(
            "POST",
            "/sessions/libassist/save",
            "/sessions/libassist/save".into(),
            Some(json!({ "yaml": "name: review\n" })),
        ),
        call(
            "PUT",
            "/settings/sandbox-profiles/{name}",
            "/settings/sandbox-profiles/team".into(),
            Some(json!({})),
        ),
        call(
            "DELETE",
            "/settings/sandbox-profiles/{name}",
            "/settings/sandbox-profiles/team".into(),
            None,
        ),
    ]
}

/// A Run id the session routes aim at: they refuse before looking it up.
const RUN: &str = "20260101-000000-abcdef0";

struct Fixtures {
    project: String,
    site: std::path::PathBuf,
    scratch: String,
}

/// A Projet with the daemon's repo as member, a mounted page, and the
/// assistant's focus on a template.
async fn seed(daemon: &TestDaemon, scratch: &Path) -> Fixtures {
    let client = reqwest::Client::new();
    let url = daemon.url();
    let project: Value = client
        .post(format!("{url}/projects"))
        .json(&json!({ "name": "fixture" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project = project["id"].as_str().unwrap().to_string();
    let member = client
        .post(format!("{url}/projects/{project}/members"))
        .json(&json!({ "path": daemon.repo_root() }))
        .send()
        .await
        .unwrap();
    assert!(member.status().is_success(), "{}", member.status());

    let site = scratch.join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<p>hi</p>").unwrap();
    let mount = client
        .post(format!("{url}/pages"))
        .json(&json!({ "name": "site", "directory": site }))
        .send()
        .await
        .unwrap();
    assert_eq!(mount.status(), 201);
    let focus = client
        .put(format!("{url}/sessions/libassist/focus"))
        .json(&json!({ "pipeline_id": "review" }))
        .send()
        .await
        .unwrap();
    assert!(focus.status().is_success(), "{}", focus.status());
    Fixtures {
        project,
        site,
        scratch: scratch.to_string_lossy().to_string(),
    }
}

/// The mutation routes of the extracted contexts, read from their routers.
fn mutation_routes() -> BTreeSet<Route> {
    context_routes()
        .into_iter()
        .filter(|(method, _)| MUTATIONS.contains(&method.as_str()))
        .filter(|(method, path)| {
            !NOT_MUTATIONS
                .iter()
                .any(|(m, p, _)| m == method && p == path)
        })
        .collect()
}

#[tokio::test]
async fn an_extension_refusing_everything_refuses_every_mutation_route() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let fixtures = seed(&daemon, scratch.path()).await;
    let calls = calls(daemon.repo_root(), &fixtures);

    // Every mutation route has its request, and nothing else does.
    let aimed: BTreeSet<Route> = calls
        .iter()
        .map(|c| (c.method.to_string(), c.template.to_string()))
        .collect();
    assert_eq!(
        aimed,
        mutation_routes(),
        "a mutation route of a context has no request here (or one aims at no route)"
    );

    control.set(Refusal::Everything);
    let client = reqwest::Client::new();
    let mut not_refused = Vec::new();
    for c in &calls {
        let method = reqwest::Method::from_bytes(c.method.as_bytes()).unwrap();
        let mut request = client.request(method, format!("{}{}", daemon.url(), c.path));
        if let Some(body) = &c.body {
            request = request.json(body);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if status != 403 || body["error"] != "operation_refused" {
            not_refused.push(format!("{} {} → {status} {body}", c.method, c.path));
        }
    }
    assert_eq!(not_refused, Vec::<String>::new());

    // Nothing moved.
    let project = daemon
        .get_json(&format!("/projects/{}", fixtures.project))
        .await;
    assert_eq!(project["name"], "fixture");
    let pages = daemon.get_json("/pages").await;
    assert_eq!(pages.as_array().map(Vec::len), Some(1), "{pages}");
    let focus = daemon.get_json("/sessions/libassist/focus").await;
    assert_eq!(focus["pipeline_id"], "review", "{focus}");
}
