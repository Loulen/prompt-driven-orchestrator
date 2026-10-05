//! The Projets of the Operations of a Run (CORE-3): a Run's and its nodes'
//! are the Projet that owns the Run's primary repository, a creation's the one
//! owning the repository it names; none when no Projet owns it.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation};
use pdo_run::application::commands::RunCommandRequest;
use pdo_run::application::create_run::CreateRunRequest;
use pdo_run::application::session_claim::SessionClaim;
use pdo_run::application::stop_node::StopNode;
use pdo_workspace::domain::project::Project;

use crate::fakes::*;

async fn status(response: Response) -> u16 {
    let status = response.status().as_u16();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    status
}

/// A world whose Projet `p1` owns the world's repository (the primary
/// repository of the Runs it seeds), refusing everything: nothing runs, the
/// Operations are only asked about.
fn world_in_projet() -> World {
    let w = world();
    let mut project = Project::named("p1", "Alpha");
    project.members = vec![w.repo.path().to_string_lossy().into_owned()];
    w.projects.0.lock().unwrap().push(project);
    *w.host.refuse.lock().unwrap() = true;
    w
}

fn create_request(repo: &str) -> CreateRunRequest {
    serde_json::from_value(serde_json::json!({
        "pipeline": "relay",
        "input": "go",
        "target_repo": repo,
    }))
    .unwrap()
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
async fn the_operations_of_a_run_and_its_nodes_are_in_the_projet_of_its_primary_repository() {
    let w = world_in_projet();
    seed_relay(&w, "r1").await;

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
async fn the_operations_of_a_run_no_projet_owns_are_in_none() {
    let w = world();
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
async fn creating_a_run_is_in_the_projet_of_the_repository_it_names() {
    let w = world_in_projet();
    let actor = Actor::LocalOperator;
    let repo = w.repo.path().to_string_lossy().into_owned();
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere.path().to_string_lossy().into_owned();

    for request in [create_request(&repo), create_request(&elsewhere)] {
        let answer = w.runs.create_run(&actor, request, Vec::new(), None).await;
        assert_eq!(status(answer).await, 403);
    }
    // The primary of `target_repos` counts when `target_repo` is blank.
    let request: CreateRunRequest = serde_json::from_value(serde_json::json!({
        "pipeline": "relay",
        "input": "go",
        "target_repo": "  ",
        "target_repos": [{ "repo": repo }],
    }))
    .unwrap();
    let answer = w.runs.create_run(&actor, request, Vec::new(), None).await;
    assert_eq!(status(answer).await, 403);

    // Another spelling of the same directory stays in its Projet.
    for alias in [format!("{repo}/"), format!("{repo}/.")] {
        let answer = w
            .runs
            .create_run(&actor, create_request(&alias), Vec::new(), None)
            .await;
        assert_eq!(status(answer).await, 403);
    }

    let projects: Vec<Vec<String>> = asked(&w).into_iter().map(|op| op.projects).collect();
    let p1 = || vec!["p1".to_string()];
    assert_eq!(projects, vec![p1(), vec![], p1(), p1(), p1()]);
}

#[tokio::test]
async fn a_run_created_from_a_node_session_without_a_repository_is_in_its_parents_projet() {
    let w = world_in_projet();
    seed_relay(&w, "r1").await;
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
    assert_eq!(asked(&w)[0].projects, vec!["p1".to_string()]);
}

#[tokio::test]
async fn an_unreadable_workspace_puts_the_operation_in_no_projet() {
    let w = world_in_projet();
    seed_relay(&w, "r1").await;
    *w.projects.1.lock().unwrap() = true;

    assert_eq!(
        status(w.runs.start_manager(&Actor::LocalOperator, "r1").await).await,
        403
    );

    assert!(asked(&w)[0].is_outside_projects());
}
