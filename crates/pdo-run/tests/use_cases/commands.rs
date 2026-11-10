//! `RunCommand`: the Pipeline Manager's commands — the forgotten-run gate, the
//! parse, the Autorisation d'opération on what the command targets, then the
//! command; the archive and `retry_all` it triggers.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_run::application::commands::RunCommandRequest;
use pdo_run::domain::event::EventKind;

use crate::fakes::*;

fn request(json: serde_json::Value) -> RunCommandRequest {
    serde_json::from_value(json).unwrap()
}

async fn answer(response: Response) -> (u16, serde_json::Value) {
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn command(w: &World, json: serde_json::Value) -> (u16, serde_json::Value) {
    answer(
        w.runs
            .run_command(&Actor::LocalOperator, "r1".into(), request(json))
            .await,
    )
    .await
}

#[tokio::test]
async fn a_run_command_asks_about_the_run_and_a_node_command_about_its_node() {
    let w = world();
    seed_relay(&w, "r1").await;
    *w.host.refuse.lock().unwrap() = true;

    let (status, body) = command(&w, serde_json::json!({ "kind": "pause_run" })).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    let (status, _) = command(
        &w,
        serde_json::json!({ "kind": "kill_node", "node_id": "worker" }),
    )
    .await;
    assert_eq!(status, 403);

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
            Operation::new(ResourceKind::Run, Verb::Action("pause_run".into())).on("r1"),
            Operation::new(ResourceKind::Node, Verb::Action("kill_node".into())).on("r1/worker"),
        ]
    );
    assert_eq!(
        kinds(&w, "r1"),
        vec![EventKind::RunStarted],
        "nothing was written"
    );
}

#[tokio::test]
async fn a_forgotten_run_answers_410_before_the_parse_and_the_authorization() {
    let w = world();
    w.store.forgotten.lock().unwrap().insert("r1".into());
    *w.host.refuse.lock().unwrap() = true;

    let (status, _) = command(&w, serde_json::json!({ "kind": "no_such_kind" })).await;

    assert_eq!(status, 410);
    assert!(w.host.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_malformed_command_answers_400_and_asks_nothing() {
    let w = world();
    seed_relay(&w, "r1").await;

    let (status, body) = command(&w, serde_json::json!({ "kind": "no_such_kind" })).await;

    assert_eq!(status, 400, "{body}");
    assert!(w.host.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn pause_then_resume_moves_the_run() {
    let w = world();
    seed_relay(&w, "r1").await;

    let (status, body) = command(&w, serde_json::json!({ "kind": "pause_run" })).await;
    assert_eq!(status, 200, "{body}");
    assert!(kinds(&w, "r1").contains(&EventKind::RunPaused));

    let (status, body) = command(&w, serde_json::json!({ "kind": "resume_run" })).await;
    assert_eq!(status, 200, "{body}");
    assert!(kinds(&w, "r1").contains(&EventKind::RunResumed));
}

#[tokio::test]
async fn retry_all_refuses_a_live_run_and_cleanup_archives_a_finished_one() {
    let w = world();
    seed_relay(&w, "r1").await;

    // `retry_all` needs a terminal Run: nothing is archived, nothing created.
    let (status, body) = command(&w, serde_json::json!({ "kind": "retry_all" })).await;
    assert_eq!(status, 409, "{body}");
    assert!(!kinds(&w, "r1").contains(&EventKind::RunArchived));

    w.runs
        .append_event()
        .execute(&event("r1", EventKind::RunCompleted, None))
        .await
        .unwrap();
    let (status, body) = command(&w, serde_json::json!({ "kind": "cleanup_run" })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "archived");
    assert!(kinds(&w, "r1").contains(&EventKind::RunArchived));
    // The archive released the Run's page mounts, and the command was the one
    // Operation asked about: the archive itself is its consequence.
    assert_eq!(*w.pages.0.lock().unwrap(), vec!["r1".to_string()]);
    let (status, body) = command(&w, serde_json::json!({ "kind": "cleanup_run" })).await;
    assert_eq!(status, 409, "archiving twice: {body}");
}

/// Approve / « mark done » is a terminal transition like any completion: the
/// node's session is snapshot then killed, so a completed node never shows a
/// live pane. Before, only the agent's own completion reaped it — a node whose
/// `pdo complete` could not reach the daemon (a sandboxed one, say) and that a
/// human marked done kept its session alive until the one-hour backstop.
#[tokio::test]
async fn mark_node_done_reaps_the_nodes_session() {
    let w = world();
    seed_relay(&w, "r1").await;
    pdo_run::application::scheduler::advance::advance_run(&w.runs, "r1").await;
    let session = pdo_session::domain::names::node_session_name("r1", "worker", 1);
    assert!(w.tmux.live.lock().unwrap().contains(&session));
    write_out(&w, "r1", "worker");

    let (status, body) = command(
        &w,
        serde_json::json!({ "kind": "mark_node_done", "node_id": "worker", "iter": 1 }),
    )
    .await;

    assert_eq!(status, 200, "{body}");
    assert!(
        w.tmux.killed.lock().unwrap().contains(&session),
        "the marked-done node's session is reaped"
    );
    assert!(!w.tmux.live.lock().unwrap().contains(&session));
}
