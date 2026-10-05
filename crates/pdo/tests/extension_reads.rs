//! CORE-5 against the real daemon: the reads go through the Autorisation
//! d'opération. Two Projets, P1 (the daemon's repository) and P2 (another),
//! a Run in each; the test extension refuses what each test says.
//!
//! - a refused Run is absent from `GET /runs`, and `403` for its detail, its
//!   events, its artifacts, its review comments, a node's pane;
//! - `/ws` relays a Run's events only to the connections that may read it;
//! - `POST /authorize` answers the question for each Operation, without
//!   running anything;
//! - a refused Projet is absent from `GET /projects`, `403` for its detail; a
//!   refused browse of the disk is `403`;
//! - the measure: 500 Runs listed under 200 ms with an extension that allows.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use pdo::extension_api::{Operation, ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{git_init, write_pipeline};
use crate::common::{ws_text, TestDaemon};
use crate::test_extension::{Refusal, TestExtension};

/// The one-node `solo` pipeline, in a git repository.
fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let yaml = r#"name: solo
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
    write_pipeline(repo, "solo", yaml)?;
    git_init(repo)
}

async fn send(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    actor: Option<&str>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{}{path}", daemon.url()));
    if let Some(body) = body {
        request = request.json(&body);
    }
    if let Some(actor) = actor {
        request = request.header("x-pdo-test-actor", actor);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(daemon: &TestDaemon, path: &str) -> (u16, Value) {
    send(daemon, reqwest::Method::GET, path, None, None).await
}

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    send(daemon, reqwest::Method::POST, path, Some(body), None).await
}

/// The Projet named `name` owning `repo`: its id.
async fn projet(daemon: &TestDaemon, name: &str, repo: &str) -> String {
    let (status, project) = post(
        daemon,
        "/projects",
        json!({ "name": name, "members": [repo] }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

/// A Run of `solo` working in `repo`: its id.
async fn run_in(daemon: &TestDaemon, repo: &str) -> String {
    let (status, body) = post(
        daemon,
        "/runs",
        json!({ "pipeline": "solo", "input": "go", "target_repo": repo }),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["run_id"].as_str().unwrap().to_string()
}

/// The world of the Feature Path: P1 owns the daemon's repository, P2 another
/// one, a Run in each.
struct World {
    daemon: TestDaemon,
    control: crate::test_extension::Control,
    p1: String,
    p2: String,
    r1: String,
    r2: String,
    _other: tempfile::TempDir,
}

async fn world() -> World {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    git_init(other.path()).unwrap();
    let other_repo = other.path().to_string_lossy().into_owned();
    let p1 = projet(&daemon, "P1", &daemon.target_repo()).await;
    let p2 = projet(&daemon, "P2", &other_repo).await;
    let r1 = run_in(&daemon, &daemon.target_repo()).await;
    let r2 = run_in(&daemon, &other_repo).await;
    World {
        daemon,
        control,
        p1,
        p2,
        r1,
        r2,
        _other: other,
    }
}

fn run_ids(list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["run_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_refused_run_is_absent_from_the_list_and_refused_one_by_one() {
    let w = world().await;
    let (status, all) = get(&w.daemon, "/runs").await;
    assert_eq!(status, 200);
    let mut both = vec![w.r1.clone(), w.r2.clone()];
    both.sort();
    assert_eq!(run_ids(&all), both);

    w.control
        .set(Refusal::parse(&format!("run:read@{}", w.p2)).unwrap());

    let (_, listed) = get(&w.daemon, "/runs").await;
    assert_eq!(
        run_ids(&listed),
        std::slice::from_ref(&w.r1),
        "the Run of P2 is hidden"
    );
    let (status, _) = get(&w.daemon, &format!("/runs/{}", w.r1)).await;
    assert_eq!(status, 200);
    for path in [
        format!("/runs/{}", w.r2),
        format!("/runs/{}/events", w.r2),
        format!("/runs/{}/children", w.r2),
        format!("/runs/{}/artifact?node_id=worker&port=out", w.r2),
        format!("/runs/{}/review/comments", w.r2),
        format!("/runs/{}/nodes/worker/pane", w.r2),
        format!("/runs/{}/nodes/worker/prompt", w.r2),
    ] {
        let (status, body) = get(&w.daemon, &path).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert_eq!(body["error"], "operation_refused", "{path}");
        assert_eq!(body["reason"], "the test extension refuses read run");
    }

    // Asked in the Projet of each Run, as a read.
    let reads: Vec<Operation> = w
        .control
        .asked()
        .into_iter()
        .map(|(_, op)| op)
        .filter(|op| op.resource == ResourceKind::Run && op.verb == Verb::Read)
        .collect();
    assert!(reads
        .iter()
        .any(|op| op.target.as_deref() == Some(w.r2.as_str()) && op.projects == [w.p2.clone()]));
    assert!(reads
        .iter()
        .any(|op| op.target.as_deref() == Some(w.r1.as_str()) && op.projects == [w.p1.clone()]));

    // A read is never announced as applied.
    let applied_reads = w
        .control
        .events()
        .into_iter()
        .filter(|event| {
            event["type"] == "operation_applied" && event["operation"]["verb"] == "read"
        })
        .count();
    assert_eq!(applied_reads, 0);
}

#[tokio::test]
async fn a_refusal_for_one_caller_leaves_the_others_their_runs() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}~bob", w.p2)).unwrap());

    let (_, as_bob) = send(&w.daemon, reqwest::Method::GET, "/runs", None, Some("bob")).await;
    assert_eq!(run_ids(&as_bob), std::slice::from_ref(&w.r1));
    let (_, as_alice) = send(
        &w.daemon,
        reqwest::Method::GET,
        "/runs",
        None,
        Some("alice"),
    )
    .await;
    assert_eq!(run_ids(&as_alice).len(), 2);
    let path = format!("/runs/{}", w.r2);
    let (status, _) = send(&w.daemon, reqwest::Method::GET, &path, None, Some("bob")).await;
    assert_eq!(status, 403);
    let (status, _) = send(&w.daemon, reqwest::Method::GET, &path, None, Some("alice")).await;
    assert_eq!(status, 200);
}

/// The `event`s of `/ws` about `run_id` within `window`.
async fn events_about<S>(ws: &mut S, run_id: &str, window: Duration) -> Vec<Value>
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, ws.next()).await {
        let Some(text) = ws_text(&message) else {
            continue;
        };
        let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let about = value["event"]["run_id"] == run_id
            || (value["type"] == "operation_applied"
                && value["operation"]["target"]
                    .as_str()
                    .is_some_and(|t| t.starts_with(run_id)));
        if about {
            seen.push(value);
        }
    }
    seen
}

#[tokio::test]
async fn ws_relays_a_runs_events_only_to_the_connections_that_may_read_it() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}~bob", w.p2)).unwrap());
    let mut bob = w.daemon.connect_ws_as("bob").await.unwrap();
    let mut alice = w.daemon.connect_ws_as("alice").await.unwrap();

    for (run_id, name) in [(&w.r2, "renamed in P2"), (&w.r1, "renamed in P1")] {
        let (status, body) = post(
            &w.daemon,
            &format!("/runs/{run_id}/commands"),
            json!({ "kind": "rename_run", "name": name }),
        )
        .await;
        assert!(status < 300, "{status}: {body}");
    }

    let window = Duration::from_millis(1500);
    let (bob_r2, alice_r2) = tokio::join!(
        events_about(&mut bob, &w.r2, window),
        events_about(&mut alice, &w.r2, window)
    );
    assert!(bob_r2.is_empty(), "bob may not read r2: {bob_r2:?}");
    assert!(!alice_r2.is_empty(), "alice reads r2");

    // Then r1, which both may read: already sent, read it from the same streams.
    let (status, _) = post(
        &w.daemon,
        &format!("/runs/{}/commands", w.r1),
        json!({ "kind": "rename_run", "name": "again" }),
    )
    .await;
    assert!(status < 300);
    let (bob_r1, alice_r1) = tokio::join!(
        events_about(&mut bob, &w.r1, window),
        events_about(&mut alice, &w.r1, window)
    );
    assert!(!bob_r1.is_empty(), "bob reads r1");
    assert!(!alice_r1.is_empty(), "alice reads r1");
}

#[tokio::test]
async fn the_question_of_authorization_answers_each_operation_without_running_it() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("repository:read,run:create@{}", w.p2)).unwrap());
    // The creations of the world are announced off the request path: let
    // them land before counting.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = w.control.events().len();

    let (status, answer) = post(
        &w.daemon,
        "/authorize",
        json!({ "operations": [
            { "resource": "repository", "verb": "read" },
            { "resource": "run", "verb": "create", "projects": [w.p1] },
            { "resource": "run", "verb": "create", "projects": [w.p2] },
            { "resource": "run", "verb": "create" },
            { "resource": "run", "verb": "read", "target": w.r2 },
        ]}),
    )
    .await;

    assert_eq!(status, 200, "{answer}");
    assert_eq!(
        answer,
        json!({ "decisions": [
            { "decision": "deny", "reason": "the test extension refuses read repository" },
            { "decision": "allow" },
            { "decision": "deny", "reason": "the test extension refuses create run" },
            { "decision": "allow" },
            { "decision": "allow" },
        ]})
    );
    // A Run named by its target is asked in its Projet.
    let (_, last) = w.control.asked().pop().unwrap();
    assert_eq!(last.projects, std::slice::from_ref(&w.p2));
    // Nothing ran: no Run created, nothing announced.
    let (_, runs) = get(&w.daemon, "/runs").await;
    assert_eq!(runs.as_array().unwrap().len(), 2);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let announced: Vec<Value> = w.control.events()[before..]
        .iter()
        .filter(|e| e["type"] == "operation_applied")
        .cloned()
        .collect();
    assert!(announced.is_empty(), "{announced:?}");

    let (status, body) = post(
        &w.daemon,
        "/authorize",
        json!({ "operations": [{ "resource": "nope", "verb": "read" }] }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "invalid_question");
}

#[tokio::test]
async fn without_extension_the_question_allows_everything() {
    let daemon = TestDaemon::spawn(git_init).await.unwrap();
    let (status, answer) = post(
        &daemon,
        "/authorize",
        json!({ "operations": [
            { "resource": "repository", "verb": "read" },
            { "resource": "run", "verb": "create", "projects": ["p1"] },
        ]}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        answer,
        json!({ "decisions": [{ "decision": "allow" }, { "decision": "allow" }] })
    );
}

#[tokio::test]
async fn a_refused_projet_is_absent_and_a_refused_browse_is_refused() {
    let w = world().await;
    let (_, projects) = get(&w.daemon, "/projects").await;
    assert_eq!(projects.as_array().unwrap().len(), 2);
    let (status, _) = get(&w.daemon, "/fs/browse").await;
    assert_eq!(status, 200);

    w.control
        .set(Refusal::parse(&format!("project:read@{},repository:read", w.p2)).unwrap());

    let (_, projects) = get(&w.daemon, "/projects").await;
    let names: Vec<&str> = projects
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["P1"]);
    let (status, _) = get(&w.daemon, &format!("/projects/{}", w.p1)).await;
    assert_eq!(status, 200);
    let (status, body) = get(&w.daemon, &format!("/projects/{}", w.p2)).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    let (status, body) = get(&w.daemon, "/fs/browse").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["reason"], "the test extension refuses read repository");
}

/// The median of `samples`.
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// CORE-5's measure: `GET /runs` over 500 Runs (in two Projets and outside
/// any), with an extension asked about each one and allowing it. The median of
/// 7 calls, after a warm-up, printed beside the same list without extension,
/// the two daemons called in turn.
///
/// An optimized build (`cargo nextest run --release`) serves it under 200 ms.
/// A debug build is slower without any extension already (the projection of
/// 500 Runs out of SQLite, ~0.7 s): there, the extension may at most double
/// the list's own time (one question per Run, the Projet of a repository
/// worked out once per list, never once per Run).
#[tokio::test]
async fn five_hundred_runs_are_listed_under_200_ms_with_an_extension_that_allows() {
    async fn seed_runs(daemon: &TestDaemon, repos: &[String]) {
        for i in 0..500 {
            let repo = &repos[i % repos.len()];
            daemon
                .seed_run_in(&format!("run-{i:04}"), "solo", repo)
                .await;
        }
    }

    /// One `GET /runs`: how long it took, and how many Runs it listed.
    async fn list(client: &reqwest::Client, daemon: &TestDaemon) -> (Duration, usize) {
        let start = Instant::now();
        let resp = client
            .get(format!("{}/runs", daemon.url()))
            .send()
            .await
            .unwrap();
        let listed: Value = resp.json().await.unwrap();
        (start.elapsed(), listed.as_array().unwrap().len())
    }

    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let with = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let without = TestDaemon::spawn(seed).await.unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let c = tempfile::tempdir().unwrap();
    let repos: Vec<String> = [&a, &b, &c]
        .iter()
        .map(|dir| dir.path().to_string_lossy().into_owned())
        .collect();
    projet(&with, "P1", &repos[0]).await;
    projet(&with, "P2", &repos[1]).await;
    seed_runs(&with, &repos).await;
    seed_runs(&without, &repos).await;

    // A warm-up each, then the two lists interleaved, call for call: a load
    // spike on the machine (a sibling test, a build) weighs on both medians
    // alike instead of on whichever daemon was measured during it.
    let client = reqwest::Client::new();
    let (mut with_samples, mut without_samples) = (Vec::new(), Vec::new());
    for round in 0..8 {
        let (took, listed) = list(&client, &with).await;
        assert_eq!(listed, 500);
        let (took_without, listed) = list(&client, &without).await;
        assert_eq!(listed, 500);
        if round > 0 {
            with_samples.push(took);
            without_samples.push(took_without);
        }
    }
    let with_extension = median(with_samples);
    let without_extension = median(without_samples);
    let reads = control
        .asked()
        .into_iter()
        .filter(|(_, op)| op.resource == ResourceKind::Run && op.verb == Verb::Read)
        .count();
    assert_eq!(reads, 500 * 8, "one question per Run per list");

    eprintln!(
        "CORE-5 measure: GET /runs over 500 Runs: {with_extension:?} with an extension that allows, {without_extension:?} without extension (median of 7)"
    );
    if cfg!(debug_assertions) {
        assert!(
            with_extension < without_extension * 2,
            "the extension slows GET /runs over 500 Runs from {without_extension:?} to {with_extension:?}"
        );
    } else {
        assert!(
            with_extension < Duration::from_millis(200),
            "GET /runs over 500 Runs took {with_extension:?} with an extension"
        );
    }
}
