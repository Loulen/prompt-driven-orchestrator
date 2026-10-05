//! The Run's surfaces: each mutation (create, forget, the repositories, the
//! run-scoped pipeline, the manager) asks the Autorisation d'opération first,
//! with its Operation, and a refusal answers `403 operation_refused` before
//! anything is written; then what each does over the in-memory ports.

use std::collections::HashMap;

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_run::application::create_run::{CreateRunRequest, PatchRunReposRequest};
use pdo_run::application::run_pipeline::SaveRunPipelineRequest;
use pdo_run::domain::event::EventKind;

use crate::fakes::*;

async fn body(response: Response) -> (u16, serde_json::Value) {
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn create_request(w: &World) -> CreateRunRequest {
    serde_json::from_value(serde_json::json!({
        "pipeline": "relay",
        "input": "go",
        "target_repo": w.repo.path(),
    }))
    .unwrap()
}

fn on_run(verb: Verb, run_id: Option<&str>) -> Operation {
    let op = Operation::new(ResourceKind::Run, verb);
    match run_id {
        Some(id) => op.on(id),
        None => op,
    }
}

fn action(name: &str) -> Verb {
    Verb::Action(name.into())
}

/// The world's repository as a git repository holding the `relay` pipeline.
fn relay_repository(w: &World) {
    let repo = w.repo.path();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(repo.join(".gitignore"), ".pdo/\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(pipelines.join("relay.prompts")).unwrap();
    std::fs::write(pipelines.join("relay.yaml"), RELAY).unwrap();
    std::fs::write(pipelines.join("relay.prompts/worker.md"), "Do the work.\n").unwrap();
    std::fs::write(pipelines.join("relay.prompts/second.md"), "Do the rest.\n").unwrap();
}

#[tokio::test]
async fn every_mutation_of_a_run_is_refused_before_anything_is_written() {
    let w = world();
    seed_relay(&w, "r1").await;
    let before = w.store.events.lock().unwrap().len();
    *w.host.refuse.lock().unwrap() = true;
    let actor = Actor::LocalOperator;
    let runs = &w.runs;

    let answers = vec![
        body(
            runs.create_run(&actor, create_request(&w), Vec::new(), None)
                .await,
        )
        .await,
        body(runs.forget_run(&actor, "r1").await).await,
        body(
            runs.edit_repos(&actor, "r1", PatchRunReposRequest::default())
                .await,
        )
        .await,
        body(
            runs.save_run_pipeline(
                &actor,
                "r1",
                SaveRunPipelineRequest {
                    yaml: RELAY.into(),
                    prompts: HashMap::new(),
                },
            )
            .await,
        )
        .await,
        body(runs.start_manager(&actor, "r1").await).await,
        body(runs.stop_manager(&actor, "r1").await).await,
    ];
    for (status, answer) in &answers {
        assert_eq!(*status, 403, "{answer}");
        assert_eq!(answer["error"], "operation_refused");
    }
    let asked: Vec<Operation> = w
        .host
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|(_, op)| op.clone())
        .collect();
    assert_eq!(
        asked,
        vec![
            on_run(Verb::Create, None),
            on_run(Verb::Delete, Some("r1")),
            on_run(action("edit_repos"), Some("r1")),
            on_run(action("edit_pipeline"), Some("r1")),
            on_run(action("start_manager"), Some("r1")),
            on_run(action("stop_manager"), Some("r1")),
        ]
    );
    assert_eq!(
        w.store.events.lock().unwrap().len(),
        before,
        "nothing written"
    );
    assert!(
        w.tmux.opened.lock().unwrap().is_empty(),
        "no session opened"
    );
}

#[tokio::test]
async fn a_created_run_starts_in_its_repository_with_its_directory_watched() {
    let w = world();
    relay_repository(&w);
    let (status, answer) = body(
        w.runs
            .create_run(&Actor::LocalOperator, create_request(&w), Vec::new(), None)
            .await,
    )
    .await;
    assert_eq!(status, 201, "{answer}");
    let run_id = answer["run_id"].as_str().unwrap().to_string();

    let started = w
        .store
        .events
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.run_id == run_id && e.kind == EventKind::RunStarted)
        .cloned()
        .expect("a RunStarted");
    let payload = started.payload.unwrap();
    assert_eq!(payload["pipeline_name"], "relay");
    assert_eq!(payload["input"], "go");
    assert_eq!(
        payload["target_repo"],
        w.repo.path().to_string_lossy().as_ref()
    );

    let run_dir = w.repo.path().join(".pdo/runs").join(&run_id);
    assert!(run_dir.join("worktree").is_dir(), "the Run worktree is cut");
    assert_eq!(
        std::fs::read_to_string(run_dir.join("worktree/.pdo/artifacts/_input/output.md")).unwrap(),
        "go"
    );
    assert_eq!(*w.watcher.0.lock().unwrap(), vec![run_dir.clone()]);
    // The ready node spawned on the host: a session opened in the worktree.
    eventually(&w, &run_id, "worker started", |k| {
        k.contains(&EventKind::NodeStarted)
    })
    .await;
    // A Run that asks nothing of the bank still gets its (empty) delivery.
    assert_eq!(
        w.skills.delivered.lock().unwrap()[0].0,
        run_dir.join("worktree")
    );
}

#[tokio::test]
async fn a_run_is_not_created_without_a_repository_nor_from_an_unknown_pipeline() {
    let w = world();
    relay_repository(&w);
    let mut no_repo = create_request(&w);
    no_repo.target_repo = None;
    let (status, answer) = body(
        w.runs
            .create_run(&Actor::LocalOperator, no_repo, Vec::new(), None)
            .await,
    )
    .await;
    assert_eq!(status, 400);
    assert!(answer["error"]
        .as_str()
        .unwrap()
        .contains("target_repo is required"));

    let mut unknown = create_request(&w);
    unknown.pipeline = "nowhere".into();
    let (status, answer) = body(
        w.runs
            .create_run(&Actor::LocalOperator, unknown, Vec::new(), None)
            .await,
    )
    .await;
    assert_eq!(status, 400);
    assert!(answer["error"]
        .as_str()
        .unwrap()
        .contains("cannot read pipeline"));
    assert!(w.store.events.lock().unwrap().is_empty(), "no Run exists");
}

#[tokio::test]
async fn forgetting_needs_an_archived_run_then_purges_and_announces_it() {
    let w = world();
    seed_relay(&w, "r1").await;
    let (status, _) = body(w.runs.forget_run(&Actor::LocalOperator, "r1").await).await;
    assert_eq!(status, 409, "a live Run cannot be forgotten");

    for kind in [EventKind::RunCompleted, EventKind::RunArchived] {
        w.runs
            .append_event()
            .execute(&event("r1", kind, None))
            .await
            .unwrap();
    }
    let (status, answer) = body(w.runs.forget_run(&Actor::LocalOperator, "r1").await).await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["status"], "forgotten");
    assert!(kinds(&w, "r1").is_empty(), "the log is purged");
    assert!(w.store.forgotten.lock().unwrap().contains("r1"));
    assert!(w
        .bus
        .published
        .lock()
        .unwrap()
        .contains(&serde_json::json!({ "type": "run_forgotten", "run_id": "r1" })));
}

#[tokio::test]
async fn stopping_the_manager_records_it_then_kills_its_session() {
    let w = world();
    seed_relay(&w, "r1").await;
    let session = pdo_session::domain::names::manager_session_name("r1");
    w.tmux.live.lock().unwrap().insert(session.clone());

    let (status, answer) = body(w.runs.stop_manager(&Actor::LocalOperator, "r1").await).await;
    assert_eq!(status, 200);
    assert_eq!(answer, serde_json::json!({ "ok": true, "stopped": true }));
    assert!(kinds(&w, "r1").contains(&EventKind::ManagerStopped));
    assert_eq!(*w.tmux.killed.lock().unwrap(), vec![session]);

    let (status, answer) = body(w.runs.stop_manager(&Actor::LocalOperator, "r1").await).await;
    assert_eq!(status, 200, "a dead session is a calm no-op");
    assert_eq!(answer["stopped"], false);
}

#[tokio::test]
async fn a_runs_children_carry_the_cost_the_live_facts_derive() {
    let w = world();
    seed_relay(&w, "parent").await;
    let mut child = event("child", EventKind::RunStarted, None);
    child.payload = Some(serde_json::json!({
        "pipeline_name": "relay",
        "parent_run_id": "parent",
        "parent_node_id": "worker",
    }));
    w.runs.append_event().execute(&child).await.unwrap();

    let (status, answer) = body(w.runs.run_children(&Actor::LocalOperator, "parent").await).await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["run_id"], "parent");
    let group = &answer["nodes"][0];
    assert_eq!(group["node_id"], "worker");
    assert_eq!(group["children"][0]["run_id"], "child");
    assert_eq!(group["children"][0]["cost"]["usd"], 1.5);

    let (status, _) = body(w.runs.run_children(&Actor::LocalOperator, "nobody").await).await;
    assert_eq!(status, 404, "an unknown parent is not an empty list");
}
