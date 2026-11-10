//! The gestures on a node: each asks the Autorisation d'opération first, with
//! the Operation it runs, and a refusal answers `403 operation_refused` before
//! anything is written.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::fail_node::FailNode;
use pdo_run::application::release_completion::ReleaseCompletion;
use pdo_run::application::retry_node::RetryNode;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::application::skip_node::SkipNode;
use pdo_run::application::start_node::StartNode;
use pdo_run::application::stop_node::StopNode;
use pdo_run::domain::event::EventKind;

use crate::fakes::*;

fn agent() -> Actor {
    Actor::Agent {
        run_id: "r1".into(),
        node_id: "worker".into(),
    }
}

fn on_worker(action: &str) -> Operation {
    Operation::new(ResourceKind::Node, Verb::Action(action.into())).on("r1/worker")
}

async fn body(response: Response) -> (u16, serde_json::Value) {
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// A world whose `relay` Run has its `worker` running.
async fn running_worker() -> World {
    let w = world();
    seed_relay(&w, "r1").await;
    advance_run(&w.runs, "r1").await;
    w
}

/// Every gesture, as the same actor, on the same running node.
async fn every_gesture(w: &World, actor: &Actor) -> Vec<(u16, serde_json::Value)> {
    let runs = &w.runs;
    let r = || ("r1".to_string(), "worker".to_string());
    vec![
        body(
            runs.complete_node(
                actor,
                CompleteNode {
                    run_id: r().0,
                    node_id: r().1,
                    iter: 1,
                    auto: false,
                },
            )
            .await,
        )
        .await,
        body(
            runs.release_completion(
                actor,
                ReleaseCompletion {
                    run_id: r().0,
                    node_id: r().1,
                    iter: 1,
                },
            )
            .await,
        )
        .await,
        body(
            runs.fail_node(
                actor,
                FailNode {
                    run_id: r().0,
                    node_id: r().1,
                    iter: Some(1),
                    reason: "broken".into(),
                },
            )
            .await,
        )
        .await,
        body(
            runs.skip_node(
                actor,
                SkipNode {
                    run_id: r().0,
                    node_id: r().1,
                    iter: Some(1),
                    reason: "nothing".into(),
                },
            )
            .await,
        )
        .await,
        body(
            runs.start_node(
                actor,
                StartNode {
                    run_id: r().0,
                    node_id: r().1,
                },
            )
            .await,
        )
        .await,
        body(
            runs.stop_node(
                actor,
                StopNode {
                    run_id: r().0,
                    node_id: r().1,
                },
            )
            .await,
        )
        .await,
        body(
            runs.retry_node(
                actor,
                RetryNode {
                    run_id: r().0,
                    node_id: r().1,
                },
            )
            .await,
        )
        .await,
    ]
}

#[tokio::test]
async fn every_gesture_on_a_node_is_refused_before_anything_is_written() {
    let w = running_worker().await;
    *w.host.refuse.lock().unwrap() = true;
    let before = kinds(&w, "r1");

    for (status, body) in every_gesture(&w, &agent()).await {
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"], "operation_refused");
        assert_eq!(body["reason"], "the test host refuses everything");
    }

    assert_eq!(kinds(&w, "r1"), before, "a refused gesture wrote nothing");
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
        [
            "complete",
            "release_completion",
            "fail",
            "skip",
            "start",
            "stop",
            "retry"
        ]
        .map(on_worker)
        .to_vec()
    );
    assert!(w
        .host
        .asked
        .lock()
        .unwrap()
        .iter()
        .all(|(actor, _)| *actor == agent()));
}

#[tokio::test]
async fn the_agents_completion_completes_its_node_and_the_scheduler_follows() {
    let w = running_worker().await;
    write_out(&w, "r1", "worker");

    let (status, body) = body(
        w.runs
            .complete_node(
                &agent(),
                CompleteNode {
                    run_id: "r1".into(),
                    node_id: "worker".into(),
                    iter: 1,
                    auto: false,
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 200, "{body}");
    assert!(kinds(&w, "r1").contains(&EventKind::NodeCompleted));
    eventually(&w, "r1", "second spawns", |kinds| {
        kinds
            .iter()
            .filter(|k| **k == EventKind::NodeStarted)
            .count()
            == 2
    })
    .await;
    // The completing node's session is reaped by the tail.
    eventually(&w, "r1", "worker reaped", |_| {
        w.tmux
            .killed
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "pdo-r1-worker-iter-1")
    })
    .await;
}

#[tokio::test]
async fn a_completion_without_its_output_is_refused_with_its_cause() {
    let w = running_worker().await;

    let (status, body) = body(
        w.runs
            .complete_node(
                &agent(),
                CompleteNode {
                    run_id: "r1".into(),
                    node_id: "worker".into(),
                    iter: 1,
                    auto: false,
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 409, "{body}");
    assert!(
        body.to_string().contains("out"),
        "the refusal names the missing port: {body}"
    );
    assert!(!kinds(&w, "r1").contains(&EventKind::NodeCompleted));
}

#[tokio::test]
async fn the_agents_failure_fails_its_node_and_parks_the_run() {
    let w = running_worker().await;

    let (status, _) = body(
        w.runs
            .fail_node(
                &agent(),
                FailNode {
                    run_id: "r1".into(),
                    node_id: "worker".into(),
                    iter: Some(1),
                    reason: "broken".into(),
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 200);
    assert!(kinds(&w, "r1").contains(&EventKind::NodeFailed));
    eventually(&w, "r1", "the run waits on its user", |kinds| {
        kinds.contains(&EventKind::RunInterrupted)
    })
    .await;
}

#[tokio::test]
async fn a_skip_completes_the_node_and_ends_the_run_skipped() {
    let w = running_worker().await;

    let (status, body) = body(
        w.runs
            .skip_node(
                &agent(),
                SkipNode {
                    run_id: "r1".into(),
                    node_id: "worker".into(),
                    iter: Some(1),
                    reason: "nothing".into(),
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["skipped"], true);
    eventually(&w, "r1", "the run ends skipped", |kinds| {
        kinds.contains(&EventKind::RunSkipped)
    })
    .await;
}

#[tokio::test]
async fn stop_stops_a_running_node_and_kills_its_session() {
    let w = running_worker().await;

    let (status, _) = body(
        w.runs
            .stop_node(
                &Actor::LocalOperator,
                StopNode {
                    run_id: "r1".into(),
                    node_id: "worker".into(),
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 200);
    assert!(kinds(&w, "r1").contains(&EventKind::NodeStopped));
    assert!(w
        .tmux
        .killed
        .lock()
        .unwrap()
        .iter()
        .any(|s| s == "pdo-r1-worker-iter-1"));
}

#[tokio::test]
async fn a_gesture_on_an_unknown_run_is_refused_by_name() {
    let w = world();

    let (status, body) = body(
        w.runs
            .stop_node(
                &Actor::LocalOperator,
                StopNode {
                    run_id: "ghost".into(),
                    node_id: "worker".into(),
                },
            )
            .await,
    )
    .await;

    assert_eq!(status, 404);
    assert_eq!(body["error"], "run not found");
}
