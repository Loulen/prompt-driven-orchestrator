//! CORE-3 against the real daemon: the catalogue of the Ressources
//! (`GET /resources`, the core's then each extension's declarations) and the
//! Projets an Operation is in, as the test extension receives them (its
//! `GET /ext/test/operations`).

use pdo::extension_api::{core_resources, Actor, Operation, ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{git_init, wait_node_status, write_pipeline};
use crate::common::TestDaemon;
use crate::test_extension::interop::Peer;
use crate::test_extension::{self, Refusal, TestExtension};

/// The daemon's repo holds a git history and the one-node `solo` pipeline.
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

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A Projet owning the daemon's repository; its id.
async fn projet_of_the_daemons_repo(daemon: &TestDaemon) -> String {
    let (status, project) = post(
        daemon,
        "/projects",
        json!({ "name": "alpha", "members": [daemon.target_repo()] }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

/// `POST /runs` of `solo` on `repo`, launched in `project`: the status and
/// the run id, if any.
async fn create_run_on(
    daemon: &TestDaemon,
    repo: &str,
    project: Option<&str>,
) -> (u16, Option<String>) {
    let (status, body) = post(
        daemon,
        "/runs",
        json!({ "pipeline": "solo", "input": "go", "target_repo": repo, "project_id": project }),
    )
    .await;
    (status, body["run_id"].as_str().map(str::to_string))
}

#[tokio::test]
async fn the_catalogue_lists_the_cores_resources_then_each_extensions() {
    let (ext, _) = TestExtension::new(Refusal::Nothing);
    let (peers, _) = Peer::trio();
    let mut extensions = ext.boxed();
    extensions.extend(peers);
    let daemon = TestDaemon::spawn_with_extensions(git_init, extensions)
        .await
        .unwrap();

    let catalogue = daemon.get_json("/resources").await;

    let catalogue = catalogue.as_array().unwrap();
    let core = core_resources();
    assert_eq!(catalogue.len(), core.len() + 4, "{catalogue:?}");
    for (served, resource) in catalogue.iter().zip(&core) {
        assert_eq!(*served, serde_json::to_value(resource).unwrap());
    }
    let run = &catalogue[0];
    assert_eq!(run["resource"], "run");
    assert_eq!(run["label"], "Runs");
    assert_eq!(run["extension"], Value::Null);
    assert!(run["verbs"].as_array().unwrap().contains(&json!("create")));
    let extensions: Vec<&Value> = catalogue[core.len()..].iter().collect();
    assert_eq!(
        *extensions[0],
        json!({
            "resource": "test/note",
            "label": "Test notes",
            "description": "The notes of the test extension",
            "verbs": ["create", "update", "delete", "pin"],
            "extension": "test",
        })
    );
    let rest: Vec<&str> = extensions[1..]
        .iter()
        .map(|r| r["resource"].as_str().unwrap())
        .collect();
    assert_eq!(rest, ["a/item", "b/item", "c/item"]);
}

#[tokio::test]
async fn without_extension_the_catalogue_is_the_cores() {
    let daemon = TestDaemon::spawn(git_init).await.unwrap();

    let catalogue = daemon.get_json("/resources").await;

    assert_eq!(catalogue, serde_json::to_value(core_resources()).unwrap());
    assert_eq!(catalogue.as_array().unwrap().len(), 19);
}

#[tokio::test]
async fn creating_a_run_reaches_the_extension_in_the_projet_it_names_or_in_none() {
    // Refused: the Operations are asked about, and nothing runs.
    let (ext, control) = TestExtension::new(Refusal::Only(ResourceKind::Run, Verb::Create));
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let projet = projet_of_the_daemons_repo(&daemon).await;
    let outside = tempfile::tempdir().unwrap();
    git_init(outside.path()).unwrap();

    let (status, _) = create_run_on(&daemon, &daemon.target_repo(), Some(&projet)).await;
    assert_eq!(status, 403);
    // ADR-0087: a repository of the Projet, launched in none, is in none.
    let (status, _) = create_run_on(&daemon, &daemon.target_repo(), None).await;
    assert_eq!(status, 403);
    let _ = &outside;

    let creations: Vec<Operation> = control
        .seen()
        .into_iter()
        .map(|(_, op)| op)
        .filter(|op| op.resource == ResourceKind::Run)
        .collect();
    let creation = Operation::new(ResourceKind::Run, Verb::Create);
    assert_eq!(
        creations,
        vec![creation.clone().in_projects([projet.as_str()]), creation,]
    );

    // The same, read where a Feature Path reads it.
    let served = daemon
        .get_json(&format!("/ext/{}/operations", test_extension::ID))
        .await;
    let runs: Vec<&Value> = served
        .as_array()
        .unwrap()
        .iter()
        .filter(|seen| seen["operation"]["resource"] == "run")
        .collect();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["operation"]["projects"], json!([projet]));
    assert_eq!(runs[0]["actor"], json!({ "kind": "local_operator" }));
    assert_eq!(runs[1]["operation"]["projects"], json!([]));
}

#[tokio::test]
async fn a_runs_node_and_session_operations_are_in_its_projet() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let projet = projet_of_the_daemons_repo(&daemon).await;
    let (status, run_id) = create_run_on(&daemon, &daemon.target_repo(), Some(&projet)).await;
    assert_eq!(status, 201);
    let run_id = run_id.unwrap();
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    // Refused from here on: asked about, nothing changes.
    control.set(Refusal::Everything);
    let (status, _) = post(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        json!({ "kind": "rename_run", "name": "renamed" }),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = post(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/stop"),
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = post(&daemon, &format!("/sessions/{run_id}/shell"), json!({})).await;
    assert_eq!(status, 403);

    let seen: Vec<(Actor, Operation)> = control.seen();
    let after_create: Vec<&Operation> = seen
        .iter()
        .map(|(_, op)| op)
        .skip_while(|op| op.resource != ResourceKind::Run)
        .collect();
    let resources: Vec<String> = after_create
        .iter()
        .map(|op| op.resource.to_string())
        .collect();
    assert_eq!(
        resources,
        ["run", "run", "node", "session"],
        "{after_create:?}"
    );
    for op in after_create {
        assert_eq!(op.projects, vec![projet.clone()], "{op:?}");
    }
}

#[tokio::test]
async fn an_extension_fills_the_projets_of_its_own_operations() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let (peer, _) = Peer::new("a");
    let mut extensions = ext.boxed();
    extensions.push(Box::new(peer));
    let daemon = TestDaemon::spawn_with_extensions(git_init, extensions)
        .await
        .unwrap();

    let (status, item) = post(
        &daemon,
        "/ext/a/items",
        json!({ "name": "one", "projects": ["p1", "p2"] }),
    )
    .await;
    assert_eq!(status, 201, "{item}");

    let (_, creation) = control.seen().pop().unwrap();
    assert_eq!(creation.resource, ResourceKind::extension("a", "item"));
    assert_eq!(creation.projects, vec!["p1".to_string(), "p2".to_string()]);
}

#[tokio::test]
async fn the_cost_cap_of_a_loop_is_a_run_operation_with_the_same_right_as_its_lap_cap() {
    let catalogue = core_resources();
    let run = catalogue.iter().find(|r| r.resource == "run").unwrap();
    for verb in ["set_region_max_iter", "set_region_max_cost"] {
        assert!(
            run.verbs.iter().any(|v| v == verb),
            "{verb}: {:?}",
            run.verbs
        );
    }

    // Refused: the extension is asked about it as a Run Operation, before the
    // command reads the pipeline, and nothing is recorded.
    let (ext, control) = TestExtension::new(Refusal::Only(
        ResourceKind::Run,
        Verb::Action("set_region_max_cost".into()),
    ));
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let (status, run_id) = create_run_on(&daemon, &daemon.target_repo(), None).await;
    assert_eq!(status, 201);
    let run_id = run_id.unwrap();

    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        json!({ "kind": "set_region_max_cost", "region_id": "any", "max_cost_usd": 5 }),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let asked = Operation::new(
        ResourceKind::Run,
        Verb::Action("set_region_max_cost".into()),
    );
    assert!(
        control
            .seen()
            .iter()
            .any(|(_, op)| op.resource == asked.resource && op.verb == asked.verb),
        "{:?}",
        control.seen()
    );
    // Its sibling is not refused: the daemon gets to the pipeline (no such region).
    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        json!({ "kind": "set_region_max_iter", "region_id": "any", "max_iter": 5 }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
}
