//! The Projet of a Run (ADR-0087): chosen at launch, recorded on the Run and
//! read by every Operation on it and its nodes (CORE-3); a creation is in the
//! Projet it names, a child inherits its parent's; none otherwise. Changing
//! the members of a Projet moves no Run.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation};
use pdo_run::application::commands::RunCommandRequest;
use pdo_run::application::create_run::CreateRunRequest;
use pdo_run::application::session_claim::SessionClaim;
use pdo_run::application::stop_node::StopNode;
use pdo_run::domain::event::EventKind;
use pdo_workspace::domain::project::Project;

use crate::fakes::*;

async fn status(response: Response) -> u16 {
    body(response).await.0
}

async fn body(response: Response) -> (u16, serde_json::Value) {
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// A world whose repository (the primary repository of the Runs it seeds) is
/// a member of both `p1` and `p2` (ADR-0087).
fn world_with_shared_repo() -> World {
    let w = world();
    let repo = w.repo.path().to_string_lossy().into_owned();
    for (id, name) in [("p1", "Alpha"), ("p2", "Beta")] {
        let mut project = Project::named(id, name);
        project.members = vec![repo.clone()];
        w.projects.0.lock().unwrap().push(project);
    }
    w
}

/// [`world_with_shared_repo`], refusing everything: nothing runs, the
/// Operations are only asked about.
fn world_in_projet() -> World {
    let w = world_with_shared_repo();
    *w.host.refuse.lock().unwrap() = true;
    w
}

fn create_request(repo: &str) -> CreateRunRequest {
    create_request_in(repo, None)
}

fn create_request_in(repo: &str, project: Option<&str>) -> CreateRunRequest {
    serde_json::from_value(serde_json::json!({
        "pipeline": "relay",
        "input": "go",
        "target_repo": repo,
        "project_id": project,
    }))
    .unwrap()
}

/// The `project_id` the `RunStarted` of `run_id` recorded.
fn recorded_project(w: &World, run_id: &str) -> serde_json::Value {
    w.store
        .events
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.run_id == run_id && e.kind == EventKind::RunStarted)
        .and_then(|e| e.payload.clone())
        .expect("a RunStarted")["project_id"]
        .clone()
}

fn asked(w: &World) -> Vec<Operation> {
    w.host
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|(_, op)| op.clone())
        .collect()
}

/// The gestures on the Run `r1`: on the Run itself, then on its node.
async fn gestures_on_r1(w: &World) {
    let actor = Actor::LocalOperator;
    assert_eq!(status(w.runs.forget_run(&actor, "r1").await).await, 403);
    assert_eq!(status(w.runs.start_manager(&actor, "r1").await).await, 403);
    let command: RunCommandRequest =
        serde_json::from_value(serde_json::json!({ "kind": "rename_run", "name": "x" })).unwrap();
    assert_eq!(
        status(w.runs.run_command(&actor, "r1".into(), command).await).await,
        403
    );
    let stop = StopNode {
        run_id: "r1".into(),
        node_id: "worker".into(),
    };
    assert_eq!(status(w.runs.stop_node(&actor, stop).await).await, 403);
    let command: RunCommandRequest =
        serde_json::from_value(serde_json::json!({ "kind": "kill_node", "node_id": "worker" }))
            .unwrap();
    assert_eq!(
        status(w.runs.run_command(&actor, "r1".into(), command).await).await,
        403
    );
}

#[tokio::test]
async fn the_operations_of_a_run_and_its_nodes_are_in_the_projet_recorded_on_it() {
    let w = world_in_projet();
    seed_relay_in(&w, "r1", Some("p1")).await;

    gestures_on_r1(&w).await;

    let asked = asked(&w);
    assert_eq!(asked.len(), 5, "{asked:?}");
    let resources: Vec<String> = asked.iter().map(|op| op.resource.to_string()).collect();
    assert_eq!(resources, ["run", "run", "run", "node", "node"]);
    for op in &asked {
        assert_eq!(op.projects, vec!["p1".to_string()], "{op:?}");
    }
}

#[tokio::test]
async fn the_operations_of_a_run_launched_in_no_projet_are_in_none() {
    // Its repository is in two Projets: the Run is in neither.
    let w = world_in_projet();
    *w.host.refuse.lock().unwrap() = true;
    seed_relay(&w, "r1").await;

    gestures_on_r1(&w).await;

    let asked = asked(&w);
    assert_eq!(asked.len(), 5, "{asked:?}");
    assert!(
        asked.iter().all(Operation::is_outside_projects),
        "{asked:?}"
    );
}

#[tokio::test]
async fn creating_a_run_is_in_the_projet_it_names_never_in_its_repositorys() {
    let w = world_in_projet();
    let actor = Actor::LocalOperator;
    let repo = w.repo.path().to_string_lossy().into_owned();

    for request in [
        create_request_in(&repo, Some("p2")),
        create_request_in(&repo, Some(" p1 ")),
        create_request(&repo),
        create_request_in(&repo, Some("  ")),
    ] {
        let answer = w.runs.create_run(&actor, request, Vec::new(), None).await;
        assert_eq!(status(answer).await, 403);
    }

    let projects: Vec<Vec<String>> = asked(&w).into_iter().map(|op| op.projects).collect();
    let p = |id: &str| vec![id.to_string()];
    assert_eq!(projects, vec![p("p2"), p("p1"), vec![], vec![]]);
}

#[tokio::test]
async fn a_run_created_from_a_node_session_is_in_its_parents_projet() {
    let w = world_in_projet();
    seed_relay_in(&w, "r1", Some("p2")).await;
    let request: CreateRunRequest =
        serde_json::from_value(serde_json::json!({ "pipeline": "relay", "input": "go" })).unwrap();
    let claim = SessionClaim {
        run_id: "r1".into(),
        node_id: "worker".into(),
    };

    let answer = w
        .runs
        .create_run(&Actor::LocalOperator, request, Vec::new(), Some(claim))
        .await;

    assert_eq!(status(answer).await, 403);
    assert_eq!(asked(&w)[0].projects, vec!["p2".to_string()]);
}

#[tokio::test]
async fn changing_the_members_of_a_projet_moves_no_run() {
    let w = world_in_projet();
    seed_relay_in(&w, "r1", Some("p1")).await;
    w.projects
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|p| p.members.clear());

    assert_eq!(
        status(w.runs.start_manager(&Actor::LocalOperator, "r1").await).await,
        403
    );

    assert_eq!(asked(&w)[0].projects, vec!["p1".to_string()]);
}

#[tokio::test]
async fn a_launch_records_the_projet_it_names_and_provisions_from_it() {
    let w = world_with_shared_repo();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();
    let actor = Actor::LocalOperator;

    let (code, p1) = body(
        w.runs
            .create_run(
                &actor,
                create_request_in(&repo, Some("p1")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 201, "{p1}");
    let (code, p2) = body(
        w.runs
            .create_run(
                &actor,
                create_request_in(&repo, Some("p2")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 201, "{p2}");
    let (code, none) = body(
        w.runs
            .create_run(&actor, create_request(&repo), Vec::new(), None)
            .await,
    )
    .await;
    assert_eq!(code, 201, "{none}");

    let id = |answer: &serde_json::Value| answer["run_id"].as_str().unwrap().to_string();
    assert_eq!(recorded_project(&w, &id(&p1)), "p1");
    assert_eq!(recorded_project(&w, &id(&p2)), "p2");
    assert_eq!(
        recorded_project(&w, &id(&none)),
        serde_json::Value::Null,
        "recorded as none, so the migration never revisits it"
    );
    assert_eq!(
        *w.projects.2.lock().unwrap(),
        vec![Some("p1".to_string()), Some("p2".to_string()), None]
    );
    let summary = w.store.summaries.lock().unwrap().clone();
    let summary_of = |run: &str| summary.get(run).and_then(|s| s.project_id.clone());
    assert_eq!(summary_of(&id(&p2)).as_deref(), Some("p2"));
    assert_eq!(summary_of(&id(&none)), None);
}

#[tokio::test]
async fn a_launch_names_an_existing_projet_holding_its_primary_repository() {
    let w = world_with_shared_repo();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere.path().to_string_lossy().into_owned();
    let actor = Actor::LocalOperator;
    let before = w.store.events.lock().unwrap().len();

    let (code, answer) = body(
        w.runs
            .create_run(
                &actor,
                create_request_in(&repo, Some("nope")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 400, "{answer}");
    assert!(answer["error"]
        .as_str()
        .unwrap()
        .contains("unknown project"));

    let (code, answer) = body(
        w.runs
            .create_run(
                &actor,
                create_request_in(&elsewhere, Some("p1")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 400, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .unwrap()
            .contains("not a member of project `Alpha`"),
        "{answer}"
    );
    assert_eq!(
        w.store.events.lock().unwrap().len(),
        before,
        "nothing written"
    );

    // Another spelling of a member is a member.
    let (code, answer) = body(
        w.runs
            .create_run(
                &actor,
                create_request_in(&format!("{repo}/"), Some("p1")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 201, "{answer}");
}

#[tokio::test]
async fn a_child_run_records_the_projet_of_its_parent() {
    let w = world_with_shared_repo();
    relay_repository(&w);
    seed_relay_in(&w, "parent", Some("p2")).await;
    w.runs
        .append_event()
        .execute(&event("parent", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    let request: CreateRunRequest = serde_json::from_value(serde_json::json!({
        "pipeline": "relay",
        "input": "go",
        "target_repo": w.repo.path(),
    }))
    .unwrap();
    let claim = SessionClaim {
        run_id: "parent".into(),
        node_id: "worker".into(),
    };

    let (code, child) = body(
        w.runs
            .create_run(&Actor::LocalOperator, request, Vec::new(), Some(claim))
            .await,
    )
    .await;

    assert_eq!(code, 201, "{child}");
    assert_eq!(
        recorded_project(&w, child["run_id"].as_str().unwrap()),
        "p2"
    );
}

#[tokio::test]
async fn a_child_run_naming_another_projet_is_refused() {
    let w = world_with_shared_repo();
    relay_repository(&w);
    seed_relay_in(&w, "parent", Some("p2")).await;
    w.runs
        .append_event()
        .execute(&event("parent", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    let claim = SessionClaim {
        run_id: "parent".into(),
        node_id: "worker".into(),
    };
    let repo = w.repo.path().to_string_lossy().into_owned();

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request_in(&repo, Some("p1")),
                Vec::new(),
                Some(claim),
            )
            .await,
    )
    .await;

    assert_eq!(code, 400, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .unwrap()
            .contains("stays in its parent's project"),
        "{answer}"
    );
}

#[tokio::test]
async fn a_projet_deleted_since_a_trigger_or_a_retry_named_it_launches_in_none() {
    // `start_run` is the creation a Trigger fire and a retry go through: the
    // Projet they carry may have been deleted since.
    let w = world_with_shared_repo();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();

    let run_id = w
        .runs
        .start_run(
            create_request_in(&repo, Some("prj-deleted")),
            Vec::new(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(recorded_project(&w, &run_id), serde_json::Value::Null);
}
