//! Stage 4 (ADR-0080) — the extension API against the real daemon (#930): the
//! test extension identifies the caller and refuses Operations; the daemon
//! answers `403` with the reason, survives a panicking extension, never asks
//! about the scheduler's own transitions, and refuses to start with an extension
//! it cannot honour.

use std::net::SocketAddr;

use pdo::extension_api::{Actor, Operation, ResourceKind, Verb};
use pdo::{serve, Config};
use serde_json::{json, Value};

use crate::common::flow::{
    complete_node, create_run, git_init, wait_node_status, wait_run_status, write_pipeline,
};
use crate::common::TestDaemon;
use crate::test_extension::{self, Refusal, TestExtension, ACTOR_HEADER};

async fn daemon(refusal: Refusal) -> (TestDaemon, test_extension::Control) {
    let (ext, control) = TestExtension::new(refusal);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    (daemon, control)
}

/// Status and JSON body (`Null` when not JSON).
async fn answer(resp: reqwest::Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn create_project(daemon: &TestDaemon, name: &str) -> (u16, Value) {
    answer(
        reqwest::Client::new()
            .post(format!("{}/projects", daemon.url()))
            .json(&json!({ "name": name }))
            .send()
            .await
            .unwrap(),
    )
    .await
}

async fn rename_project(daemon: &TestDaemon, id: &str, name: &str) -> (u16, Value) {
    answer(
        reqwest::Client::new()
            .patch(format!("{}/projects/{id}", daemon.url()))
            .json(&json!({ "name": name }))
            .send()
            .await
            .unwrap(),
    )
    .await
}

fn refusal_body(operation: &Operation) -> Value {
    let reason = test_extension::reason(operation);
    json!({
        "error": "operation_refused",
        "message": format!("operation refused: {reason}"),
        "reason": reason,
        "sentence": test_extension::sentence(operation),
    })
}

#[tokio::test]
async fn refusing_project_updates_refuses_the_rename_and_lets_the_creation_through() {
    let (daemon, _) = daemon(Refusal::Only(ResourceKind::Project, Verb::Update)).await;

    let (status, project) = create_project(&daemon, "demo").await;
    assert_eq!(status, 201, "the creation passes: {project}");
    let id = project["id"].as_str().unwrap().to_string();

    let (status, body) = rename_project(&daemon, &id, "renamed").await;
    assert_eq!(status, 403);
    assert_eq!(
        body,
        refusal_body(&Operation::new(ResourceKind::Project, Verb::Update))
    );

    // Refused before any effect.
    let stored = daemon.get_json(&format!("/projects/{id}")).await;
    assert_eq!(stored["name"], "demo");
}

/// The pencil's "name this group" gesture: the initial members travel with the
/// name, so refusing only `Update` still lets a whole Projet be created (FP #930).
#[tokio::test]
async fn refusing_project_updates_still_creates_a_projet_with_its_initial_members() {
    let (daemon, _) = daemon(Refusal::Only(ResourceKind::Project, Verb::Update)).await;

    let (status, project) = answer(
        reqwest::Client::new()
            .post(format!("{}/projects", daemon.url()))
            .json(&json!({ "name": "demo", "members": ["/repos/front"] }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 201, "the creation passes: {project}");
    assert_eq!(project["members"], json!(["/repos/front"]));
    let id = project["id"].as_str().unwrap();
    let stored = daemon.get_json(&format!("/projects/{id}")).await;
    assert_eq!(stored["members"], json!(["/repos/front"]));
}

#[tokio::test]
async fn the_actor_identify_sets_is_the_one_authorize_receives() {
    let (daemon, control) = daemon(Refusal::Nothing).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/projects", daemon.url()))
        .header(ACTOR_HEADER, "alice")
        .json(&json!({ "name": "hers" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let (status, _) = create_project(&daemon, "mine").await;
    assert_eq!(status, 201);

    let creation = Operation::new(ResourceKind::Project, Verb::Create);
    assert_eq!(
        control.seen(),
        vec![
            (
                Actor::Identified {
                    namespace: test_extension::ID.into(),
                    id: "alice".into()
                },
                creation.clone()
            ),
            (Actor::LocalOperator, creation),
        ]
    );
}

#[tokio::test]
async fn a_panic_in_authorize_is_a_refusal_and_the_daemon_lives_on() {
    let (daemon, control) = daemon(Refusal::Nothing).await;
    let (_, project) = create_project(&daemon, "demo").await;
    let id = project["id"].as_str().unwrap().to_string();

    control.set(Refusal::Panic);
    for _ in 0..2 {
        let (status, body) = rename_project(&daemon, &id, "renamed").await;
        assert_eq!(status, 403);
        assert_eq!(body["error"], "operation_refused");
        assert_eq!(body["reason"], "extension `test` failed");
    }

    // Still serving, reads and mutations alike.
    assert_eq!(
        daemon.get_json(&format!("/projects/{id}")).await["name"],
        "demo"
    );
    control.set(Refusal::Nothing);
    let (status, renamed) = rename_project(&daemon, &id, "renamed").await;
    assert_eq!(status, 200, "{renamed}");
    assert_eq!(renamed["name"], "renamed");
}

/// `start → worker → second → end`: two agent nodes in a row.
fn seed_relay(repo: &std::path::Path) -> anyhow::Result<()> {
    let yaml = r#"name: relay
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
  - id: second
    name: Second
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
    target: { node: second, port: task }
  - source: { node: second, port: out }
    target: { node: end, port: result }
"#;
    write_pipeline(repo, "relay", yaml)?;
    std::fs::write(
        repo.join(".pdo/pipelines/relay.prompts/second.md"),
        "Do the rest.\n",
    )?;
    git_init(repo)
}

fn complete(run_id: &str, node_id: &str) -> Operation {
    Operation::new(ResourceKind::Node, Verb::Action("complete".into()))
        .on(format!("{run_id}/{node_id}"))
}

#[tokio::test]
async fn the_scheduler_never_asks_the_authorization() {
    // The extension refuses everything while the Run is in flight: an agent's
    // completion is a gesture and is refused, but the scheduler still drives the
    // Run — the next node spawns and the Run completes — because none of its
    // transitions is an Operation.
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed_relay, ext.boxed())
        .await
        .unwrap();
    let run_id = create_run(&daemon, "relay").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let before = control.seen().len();

    control.set(Refusal::Everything);
    let (status, body) = complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    assert_eq!(
        wait_node_status(&daemon, &run_id, "worker", "running").await["status"],
        "running",
        "a refused completion leaves the Run as it was"
    );

    control.set(Refusal::Nothing);
    let (status, body) = complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 200, "worker completes: {body}");
    // Everything refused again: the spawn of `second` is the scheduler's.
    control.set(Refusal::Everything);
    wait_node_status(&daemon, &run_id, "second", "running").await;

    control.set(Refusal::Nothing);
    let (status, body) = complete_node(&daemon, &run_id, "second").await;
    assert_eq!(status, 200, "second completes: {body}");
    wait_run_status(&daemon, &run_id, "completed").await;

    // Only this test's three completions were asked about.
    let asked: Vec<Operation> = control
        .seen()
        .into_iter()
        .skip(before)
        .map(|(_, op)| op)
        .collect();
    assert_eq!(
        asked,
        vec![
            complete(&run_id, "worker"),
            complete(&run_id, "worker"),
            complete(&run_id, "second"),
        ],
        "a scheduler transition asked the authorization"
    );
}

#[tokio::test]
async fn an_extension_requiring_another_api_is_refused_at_startup() {
    let repo = tempfile::tempdir().unwrap();
    let (ext, _) = TestExtension::new(Refusal::Nothing);
    let refused = serve(
        Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), repo.path()),
        ext.requiring_api("99.0.0").boxed(),
    )
    .await;

    let err = format!("{:#}", refused.err().expect("the daemon refuses to start"));
    assert!(
        err.contains(&format!(
            "extension `test` requires extension API 99.0.0, but this PDO implements \
             extension API {}",
            pdo::extension_api::API_VERSION
        )),
        "{err}"
    );
    // Refused before opening anything.
    assert!(!repo.path().join(".pdo").exists());
}
