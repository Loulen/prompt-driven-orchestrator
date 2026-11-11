//! The Projet of a Run (ADR-0087): chosen at launch, recorded on the Run and
//! read by every Operation on it and its nodes (CORE-3); a creation is in the
//! Projet it names, else in the one holding its repository (refused when
//! several do), a child inherits its parent's and works in its repositories;
//! none otherwise, the disk read then. Changing the members of a Projet moves
//! no Run.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
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
async fn creating_a_run_is_in_the_projet_it_names() {
    let w = world_in_projet();
    let actor = Actor::LocalOperator;
    let repo = w.repo.path().to_string_lossy().into_owned();

    for request in [
        create_request_in(&repo, Some("p2")),
        create_request_in(&repo, Some(" p1 ")),
    ] {
        let answer = w.runs.create_run(&actor, request, Vec::new(), None).await;
        assert_eq!(status(answer).await, 403);
    }

    let projects: Vec<Vec<String>> = asked(&w).into_iter().map(|op| op.projects).collect();
    let p = |id: &str| vec![id.to_string()];
    assert_eq!(projects, vec![p("p2"), p("p1")]);
}

/// A world whose repository is a member of `p1` only.
fn world_with_repo_in_p1() -> World {
    let w = world_with_shared_repo();
    w.projects
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|p| p.id == "p2")
        .for_each(|p| p.members.clear());
    w
}

#[tokio::test]
async fn a_launch_naming_no_projet_is_in_the_one_holding_its_repository() {
    let w = world_with_repo_in_p1();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();

    // Another spelling of the member is the member.
    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&format!("{repo}/")),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;

    assert_eq!(code, 201, "{answer}");
    assert_eq!(
        recorded_project(&w, answer["run_id"].as_str().unwrap()),
        "p1"
    );
    // Asked in that Projet, and the disk is not read.
    let asked = asked(&w);
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].resource.to_string(), "run");
    assert_eq!(asked[0].projects, vec!["p1".to_string()]);
}

#[tokio::test]
async fn a_launch_naming_no_projet_is_judged_in_the_one_holding_its_repository() {
    let w = world_with_repo_in_p1();
    *w.host.refuse.lock().unwrap() = true;
    let repo = w.repo.path().to_string_lossy().into_owned();

    let answer = w
        .runs
        .create_run(
            &Actor::LocalOperator,
            create_request(&repo),
            Vec::new(),
            None,
        )
        .await;

    assert_eq!(status(answer).await, 403);
    assert_eq!(asked(&w)[0].projects, vec!["p1".to_string()]);
}

#[tokio::test]
async fn a_repository_of_several_projets_needs_its_projet_named() {
    let w = world_with_shared_repo();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();
    let before = w.store.events.lock().unwrap().len();

    for request in [create_request(&repo), create_request_in(&repo, Some("  "))] {
        let (code, answer) = body(
            w.runs
                .create_run(&Actor::LocalOperator, request, Vec::new(), None)
                .await,
        )
        .await;
        assert_eq!(code, 400, "{answer}");
        assert_eq!(
            answer["error"],
            "This repository is in Alpha and Beta: pick its Project."
        );
        assert_eq!(
            answer["projects"],
            serde_json::json!([
                { "id": "p1", "name": "Alpha" },
                { "id": "p2", "name": "Beta" },
            ])
        );
    }

    // Only which Projets the actor may read by name: no creation is asked.
    let asked = asked(&w);
    assert!(
        asked.iter().all(|op| op.resource == ResourceKind::Project),
        "{asked:?}"
    );
    assert_eq!(
        w.store.events.lock().unwrap().len(),
        before,
        "nothing written"
    );
}

#[tokio::test]
async fn the_projets_of_an_ambiguous_repository_the_actor_may_not_read_are_counted() {
    let w = world_with_shared_repo();
    w.host.refuse_reads_in.lock().unwrap().push("p2".into());
    let repo = w.repo.path().to_string_lossy().into_owned();

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&repo),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;

    assert_eq!(code, 400, "{answer}");
    assert_eq!(
        answer["error"],
        "This repository is in Alpha and another Project: pick its Project."
    );
    assert_eq!(
        answer["projects"],
        serde_json::json!([{ "id": "p1", "name": "Alpha" }])
    );
}

#[tokio::test]
async fn a_launch_on_a_repository_of_no_projet_also_reads_the_disk() {
    let w = world();
    relay_repository(&w);
    let repo = w.repo.path().to_string_lossy().into_owned();

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&repo),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;

    assert_eq!(code, 201, "{answer}");
    assert_eq!(
        recorded_project(&w, answer["run_id"].as_str().unwrap()),
        serde_json::Value::Null
    );
    let asked = asked(&w);
    assert_eq!(
        asked
            .iter()
            .map(|op| (op.resource.clone(), op.verb.clone()))
            .collect::<Vec<_>>(),
        [
            (ResourceKind::Run, Verb::Create),
            (ResourceKind::Repository, Verb::Read)
        ]
    );
    assert!(
        asked.iter().all(Operation::is_outside_projects),
        "{asked:?}"
    );
}

#[tokio::test]
async fn reading_the_disk_refused_no_run_is_launched_outside_the_projets() {
    let w = world();
    relay_repository(&w);
    *w.host.refuse_disk.lock().unwrap() = true;
    let repo = w.repo.path().to_string_lossy().into_owned();
    let before = w.store.events.lock().unwrap().len();

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&repo),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;

    assert_eq!(code, 403, "{answer}");
    assert_eq!(answer["reason"], "the test host refuses reading the disk");
    assert_eq!(w.store.events.lock().unwrap().len(), before, "no Run");
    let runs = w.repo.path().join(".pdo").join("runs");
    assert!(
        !runs.exists() || std::fs::read_dir(&runs).unwrap().next().is_none(),
        "no worktree"
    );
    let branches = std::process::Command::new("git")
        .args(["branch", "--list", "pdo/run-*"])
        .current_dir(w.repo.path())
        .output()
        .unwrap();
    assert!(branches.stdout.is_empty(), "no branch");
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
    // Out of both Projets, the repository launches in none.
    w.projects
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|p| p.members.clear());
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

/// A parent Run in `project`, its node `worker` started, and the claim of
/// that node's session.
async fn parent_in(w: &World, project: Option<&str>) -> SessionClaim {
    seed_relay_in(w, "parent", project).await;
    w.runs
        .append_event()
        .execute(&event("parent", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    SessionClaim {
        run_id: "parent".into(),
        node_id: "worker".into(),
    }
}

/// A second git repository, a member of no Projet unless one is told.
fn other_repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    dir
}

#[tokio::test]
async fn a_child_run_works_in_a_repository_of_its_parents_projet() {
    let w = world_with_repo_in_p1();
    relay_repository(&w);
    let other = other_repository();
    let other_path = other.path().to_string_lossy().into_owned();
    w.projects
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|p| p.id == "p2")
        .for_each(|p| p.members = vec![other_path.clone()]);
    let claim = parent_in(&w, Some("p1")).await;
    let before = w.store.events.lock().unwrap().len();

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&other_path),
                Vec::new(),
                Some(claim.clone()),
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
    assert_eq!(w.store.events.lock().unwrap().len(), before, "no Run");

    let repo = w.repo.path().to_string_lossy().into_owned();
    let (code, child) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&repo),
                Vec::new(),
                Some(claim),
            )
            .await,
    )
    .await;
    assert_eq!(code, 201, "{child}");
    assert_eq!(
        recorded_project(&w, child["run_id"].as_str().unwrap()),
        "p1"
    );
}

#[tokio::test]
async fn a_child_of_a_run_in_no_projet_works_in_a_repository_of_no_projet() {
    let w = world();
    relay_repository(&w);
    let other = other_repository();
    let other_path = other.path().to_string_lossy().into_owned();
    let mut project = Project::named("p1", "Alpha");
    project.members = vec![other_path.clone()];
    w.projects.0.lock().unwrap().push(project);
    let claim = parent_in(&w, None).await;

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&other_path),
                Vec::new(),
                Some(claim.clone()),
            )
            .await,
    )
    .await;
    assert_eq!(code, 400, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .unwrap()
            .contains("works in a repository of no project"),
        "{answer}"
    );

    // Its own repository, of no Projet: launched, the disk read.
    let repo = w.repo.path().to_string_lossy().into_owned();
    let (code, child) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                create_request(&repo),
                Vec::new(),
                Some(claim),
            )
            .await,
    )
    .await;
    assert_eq!(code, 201, "{child}");
    let asked = asked(&w);
    assert!(
        asked
            .iter()
            .any(|op| op.resource.to_string() == "repository" && op.is_outside_projects()),
        "{asked:?}"
    );
}

#[tokio::test]
async fn a_refused_creation_says_nothing_of_the_projet_it_names() {
    // The creation is judged first: who may not create a Run in `p1` learns
    // nothing of its members, nor its name.
    let w = world_in_projet();
    let elsewhere = other_repository();
    let request = create_request_in(&elsewhere.path().to_string_lossy(), Some("p1"));

    let (code, answer) = body(
        w.runs
            .create_run(&Actor::LocalOperator, request, Vec::new(), None)
            .await,
    )
    .await;

    assert_eq!(code, 403, "{answer}");
    assert!(!answer.to_string().contains("Alpha"), "{answer}");
}

#[tokio::test]
async fn a_secondary_repository_outside_the_projet_is_read_from_the_disk() {
    let w = world_with_repo_in_p1();
    relay_repository(&w);
    *w.host.refuse_disk.lock().unwrap() = true;
    let repo = w.repo.path().to_string_lossy().into_owned();
    let other = other_repository();
    let request = |secondary: &str| -> CreateRunRequest {
        serde_json::from_value(serde_json::json!({
            "pipeline": "relay",
            "input": "go",
            "target_repo": repo,
            "target_repos": [{ "repo": repo }, { "repo": secondary }],
        }))
        .unwrap()
    };

    let (code, answer) = body(
        w.runs
            .create_run(
                &Actor::LocalOperator,
                request(&other.path().to_string_lossy()),
                Vec::new(),
                None,
            )
            .await,
    )
    .await;
    assert_eq!(code, 403, "{answer}");
    assert_eq!(answer["reason"], "the test host refuses reading the disk");

    // A secondary of the Projet reads nothing from the disk.
    let other_path = other.path().to_string_lossy().into_owned();
    w.projects
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|p| p.id == "p1")
        .for_each(|p| p.members.push(other_path.clone()));
    let before = asked(&w).len();
    let answer = w
        .runs
        .create_run(
            &Actor::LocalOperator,
            request(&other_path),
            Vec::new(),
            None,
        )
        .await;
    let asked = asked(&w);
    assert!(
        asked[before..]
            .iter()
            .all(|op| op.resource != ResourceKind::Repository),
        "{asked:?}"
    );
    assert_ne!(status(answer).await, 403);
}
