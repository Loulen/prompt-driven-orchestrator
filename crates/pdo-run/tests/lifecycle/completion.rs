//! A node's completion: the agent's `done`, the Manager's `mark_node_done`, the release of an interactive node, and every refusal on the way.

#![allow(unused_imports)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::Router;
use pretty_assertions::assert_eq;
use tower::ServiceExt;

use pdo_pipeline::domain::pipeline;
use pdo_run::application::admission_count::count_global_live_sessions;
use pdo_run::application::boot_recovery;
use pdo_run::application::complete_node::{
    complete_node_iteration, interactive_completion_refusal, CompletionAttempt, CompletionSource,
};
use pdo_run::application::events::{emit_run_event, reload_run_state, reload_run_state_with};
use pdo_run::application::node::launch::{find_node_type, node_run_isolation, snapshot_isolation};
use pdo_run::application::node::spawn::{spawn_node, SpawnContext, SpawnDeps, SpawnOutcome};
use pdo_run::application::paths::{
    effective_repo_root, resolve_pipeline_path, run_scoped_pipeline_path,
};
use pdo_run::application::release_completion::release_node_completion;
use pdo_run::application::sandbox_prep::mark_sandbox_prep_ready;
use pdo_run::application::scheduler::node_completion::spawn_ready_after_event;
use pdo_run::application::scheduler::stall::{max_event_age_secs, reconcile_run_level_stall};
use pdo_run::application::stale::detector as stale_detector;
use pdo_run::domain::run_stall::SPAWN_STALL_GRACE_SECS;
use pdo_run::domain::scheduler::dispatcher as scheduler_dispatcher;
use pdo_run::domain::{admission, blackboard, completion_refusal, merge_action, scheduler};
use pdo_workspace::adapters::git::worktrees::{
    self as worktrees, create_secondary_snapshot, create_sub_worktree, create_worktree,
    secondary_snapshot_path, sub_worktree_branch, sub_worktree_path, worktree_dir_for_run,
    worktree_has_tracked_changes,
};
use pdo_workspace::domain::provisioning;
use tokio::time;

use crate::harness::*;
use crate::helpers::*;

#[tokio::test]
async fn node_done_and_run_lifecycle() {
    let state = test_state().await;

    // Manually append events to simulate a run
    let run_id = "test-run-1";
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test" })),
    };
    append_event(&state, &run_started).await.unwrap();

    let node_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some("worker".into()),
        iter: Some(1),
        payload: None,
    };
    append_event(&state, &node_started).await.unwrap();

    // Call node_done endpoint
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run-1/nodes/worker/done")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // Check the run reaches completed (detached tail, #304 — poll).
    let run_state = wait_run_status(&state, run_id, event_log::RunStatus::Completed).await;
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
}

#[tokio::test]
async fn mark_node_done_force_completes_unreleased_interactive_node() {
    let state = test_state().await;

    let run_id = "test-interactive-run";
    // Simulate: run_started → node_started → node_awaiting_user
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "interactive" })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "interactive": true })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    // Verify run is awaiting_user
    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::AwaitingUser);

    // Call mark_node_done
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // Verify run is completed (single-node pipeline)
    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Completed);
    assert_eq!(
        run_state.nodes["griller"].status,
        event_log::NodeStatus::Completed
    );
    assert!(
        !events
            .iter()
            .any(|event| event.kind == event_log::EventKind::NodeCompletionReleased),
        "force completion must bypass the release guard"
    );
}

#[tokio::test]
async fn mark_node_done_emits_command_issued_event() {
    let state = test_state().await;

    let run_id = "test-mnd-cmd-event";
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "interactive" })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    let cmd_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::CommandIssued)
        .collect();
    assert_eq!(cmd_events.len(), 1);
    let payload = cmd_events[0].payload.as_ref().unwrap();
    assert_eq!(payload["command"], "mark_node_done");
    assert_eq!(payload["node_id"], "griller");
    assert!(
        !events
            .iter()
            .any(|event| event.kind == event_log::EventKind::NodeCompletionReleased),
        "force completion must not require a prior release"
    );
}

#[tokio::test]
async fn every_completion_source_is_refused_until_interactive_release() {
    let state = test_state().await;
    let run_id = "interactive-completion-guard";
    seed_interactive_node(&state, run_id, event_log::EventKind::NodeAwaitingUser).await;

    for source in [
        CompletionSource::Explicit,
        CompletionSource::StopHook,
        CompletionSource::TurnEnded,
        CompletionSource::ChildrenSettled,
    ] {
        let attempt =
            complete_node_iteration(&state.run, run_id.into(), "griller".into(), 1, source).await;
        assert!(matches!(
            attempt,
            CompletionAttempt::Refused {
                refusal: completion_refusal::CompletionRefusal::CompletionNotReleased
            }
        ));
    }

    let response = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/griller/done"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iter":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["error"], "completion_not_released");
    assert_eq!(body["recoverable"], true);
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        event_log::EventKind::NodeCompleted
            | event_log::EventKind::NodeAutoCompleted
            | event_log::EventKind::NodeFailed
    )));
}

#[tokio::test]
async fn release_route_is_projection_backed_and_idempotent() {
    let state = test_state().await;
    let run_id = "interactive-release-route";
    seed_interactive_node(&state, run_id, event_log::EventKind::NodeAwaitingUser).await;

    for expected_noop in [false, true] {
        let response = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/griller/release"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"iter":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["released"], true);
        assert_eq!(
            body.get("noop").and_then(|v| v.as_bool()),
            expected_noop.then_some(true)
        );
    }

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == event_log::EventKind::NodeCompletionReleased)
            .count(),
        1
    );
    let run = event_log::project(&events).unwrap();
    assert_eq!(run.status, event_log::RunStatus::Running);
    assert_eq!(run.nodes["griller"].status, event_log::NodeStatus::Running);
    assert!(run.nodes["griller"].iterations[0].completion_released);
    assert!(interactive_completion_refusal(&run, "griller", 1).is_none());
}

#[tokio::test]
async fn release_command_uses_the_same_operation() {
    let state = test_state().await;
    let run_id = "interactive-release-command";
    seed_interactive_node(&state, run_id, event_log::EventKind::NodeAwaitingUser).await;

    let response = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind":"release_node_completion","node_id":"griller","iter":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == event_log::EventKind::NodeCompletionReleased)
            .count(),
        1
    );
}

#[tokio::test]
async fn release_refuses_noninteractive_and_dead_sessions() {
    let state = test_state().await;
    for (run_id, interactive, terminal_node, terminal_run, expected) in [
        (
            "release-noninteractive",
            false,
            false,
            false,
            "node_not_interactive",
        ),
        (
            "release-dead-session",
            true,
            true,
            false,
            "node_session_not_live",
        ),
        ("release-dead-run", true, true, true, "run_not_live"),
    ] {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "interactive" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({ "interactive": interactive })),
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }
        if terminal_node {
            append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: run_id.into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event_log::EventKind::NodeCompleted,
                    node_id: Some("worker".into()),
                    iter: Some(1),
                    payload: None,
                },
            )
            .await
            .unwrap();
        }
        if terminal_run {
            append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: run_id.into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event_log::EventKind::RunCompleted,
                    node_id: None,
                    iter: None,
                    payload: None,
                },
            )
            .await
            .unwrap();
        }

        let response = release_node_completion(&state.run, run_id, "worker", 1, None).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response_json(response).await["error"], expected);
    }
}

#[tokio::test]
async fn node_done_emits_exactly_one_run_completed() {
    // Caller A (CompletionFirst, flag=false): a single-node run driven to
    // terminal by `pdo complete` emits `RunCompleted` exactly once.
    let state = test_state().await;
    let run_id = "c275-node-done-single";
    for ev in [
        seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "test" })),
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &ev).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The completion gate runs in the detached tail — poll to
    // terminal, then assert the exactly-once invariant on the final log.
    wait_run_status(&state, run_id, event_log::RunStatus::Completed).await;
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_run_completed(&events),
        1,
        "node_done must emit exactly one RunCompleted"
    );
}

#[tokio::test]
async fn mark_node_done_emits_exactly_one_run_completed() {
    // Caller C (SweepFirst, flag=true): an interactive node completed via the
    // `mark_node_done` command drives the run to terminal with one
    // `RunCompleted`.
    let state = test_state().await;
    let run_id = "c275-mark-single";
    for ev in [
        seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "interactive" })),
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("griller"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeAwaitingUser,
            Some("griller"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &ev).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_run_completed(&events),
        1,
        "mark_node_done must emit exactly one RunCompleted"
    );
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::Completed
    );
}

#[tokio::test]
async fn mark_node_done_completion_carries_source_payload() {
    // The head stays caller-owned: `mark_node_done` tags its `NodeCompleted`
    // with `source = "mark_node_done"`. The carve must NOT homogenize it.
    let state = test_state().await;
    let run_id = "c275-mark-source";
    for ev in [
        seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "interactive" })),
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("griller"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeAwaitingUser,
            Some("griller"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &ev).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    let nc = events
        .iter()
        .find(|e| {
            e.kind == event_log::EventKind::NodeCompleted && e.node_id.as_deref() == Some("griller")
        })
        .expect("griller NodeCompleted present");
    assert_eq!(
        nc.payload.as_ref().expect("source payload present")["source"],
        "mark_node_done"
    );
}

#[tokio::test]
async fn node_done_completion_has_no_source_payload() {
    // The complement of the source-payload test: `node_done`'s `NodeCompleted`
    // carries no source, so the two callers stay distinguishable in the log;
    // it names only who marked the node done.
    let state = test_state().await;
    let run_id = "c275-node-done-nosrc";
    for ev in [
        seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "test" })),
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &ev).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    let nc = events
        .iter()
        .find(|e| {
            e.kind == event_log::EventKind::NodeCompleted && e.node_id.as_deref() == Some("worker")
        })
        .expect("worker NodeCompleted present");
    assert_eq!(
        nc.payload,
        Some(serde_json::json!({ "actor": { "kind": "local_operator" } })),
        "node_done's NodeCompleted carries no source payload, only who marked it done"
    );
}

#[tokio::test]
async fn awaiting_user_completion_respects_per_caller_flag() {
    // The completion flag diverges by caller: `node_done` (flag=false) must
    // NOT complete a run still projecting `AwaitingUser`; `mark_node_done`
    // (flag=true) must. A second interactive `holder` node — present in the
    // live `nodes` map but OUTSIDE the expected completion set (`node_defs`
    // lists only `worker`) — keeps the run `AwaitingUser` while the expected
    // `worker` completes, so the per-caller flag is the only thing that
    // decides completion. (The pure gate semantics are pinned in
    // `run_advance.rs`; this pins the per-caller WIRING.)
    async fn seed(state: &Arc<TestState>, run_id: &str) {
        for ev in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({
                    "pipeline_name": "interactive",
                    "node_defs": [
                        { "id": "worker", "node_type": "agent", "isolated_worktree": false, "inputs": [], "outputs": [] }
                    ],
                    "edges": []
                })),
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("holder"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeAwaitingUser,
                Some("holder"),
                Some(1),
                None,
            ),
        ] {
            append_event(state, &ev).await.unwrap();
        }
    }

    // Flag=false (node_done): does NOT complete while `holder` still awaits.
    let state = test_state().await;
    seed(&state, "c275-flag-false").await;
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/c275-flag-false/nodes/worker/done")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let events = load_events(&state.db, "c275-flag-false").await.unwrap();
    assert_eq!(
        count_run_completed(&events),
        0,
        "node_done (flag=false) must not complete an AwaitingUser run"
    );
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::AwaitingUser
    );

    // Flag=true (mark_node_done): completes despite the awaiting `holder`.
    let state = test_state().await;
    seed(&state, "c275-flag-true").await;
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/c275-flag-true/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let events = load_events(&state.db, "c275-flag-true").await.unwrap();
    assert_eq!(
        count_run_completed(&events),
        1,
        "mark_node_done (flag=true) must complete the run despite an awaiting node"
    );
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::Completed
    );
}

#[tokio::test]
async fn mark_node_done_rejects_unknown_command() {
    let state = test_state().await;

    let run_id = "test-unknown-cmd";
    let event = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test" })),
    };
    append_event(&state, &event).await.unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "nope"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mark_node_done_requires_node_id() {
    let state = test_state().await;

    let run_id = "test-no-node-id";
    let event = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test" })),
    };
    append_event(&state, &event).await.unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "mark_node_done"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mark_node_done_returns_409_when_outputs_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "validate-test";
    write_pipeline_with_outputs(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "validate-409";
    seed_awaiting_run(&state, run_id, pipe_name).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"], "missing_outputs");
    let missing = body["missing"].as_array().unwrap();
    assert!(missing.iter().any(|v| v == "summary"));
    assert!(missing.iter().any(|v| v == "report"));
}

#[tokio::test]
async fn mark_node_done_on_failed_node_embeds_reopen_and_completes() {
    // AC7 / ADR-0049 (amends #212): a human `mark_node_done` on a Failed Run
    // (its failed iter hand-fixed with the required outputs) now EMBEDS the
    // re-open — one atomic command completes the node instead of the
    // pre-résilience "resume_run first, then mark" two-step.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "failed-rescue";
    write_pipeline_with_outputs(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "failed-rescue-1";
    seed_failed_run(&state, run_id, pipe_name).await;

    // Create the required output files (directory-based, ADR-0010)
    let base = tmp
        .path()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts/worker/iter-1");
    let summary_dir = base.join("summary");
    std::fs::create_dir_all(&summary_dir).unwrap();
    std::fs::write(summary_dir.join("output.md"), "# Summary\nDone.").unwrap();
    let report_dir = base.join("report");
    std::fs::create_dir_all(&report_dir).unwrap();
    std::fs::write(report_dir.join("output.md"), "# Report\nAll good.").unwrap();

    // A SINGLE mark_node_done on the Failed run: it embeds the re-open and
    // completes the (hand-fixed) failed iteration atomically.
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    // The embedded re-open is in the log, and the previous terminal label is
    // preserved (AC12).
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued
                && e.payload.as_ref().and_then(|p| p.get("command"))
                    == Some(&serde_json::json!("reopen_run"))),
        "the mark_node_done must embed a reopen_run CommandIssued"
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the previous terminal label stays in the log (AC12)"
    );
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
}

#[tokio::test]
async fn mark_node_done_on_completed_iter_is_noop_without_downstream_spawn() {
    // The duplicate completion must neither re-emit node_completed
    // nor re-trigger downstream spawns.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-dup-mark";
    write_pipeline_with_consumer(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "guard-dup-mark-1";
    // Worker completed organically; consumer already running iter 1.
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("consumer".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["noop"], true, "duplicate completion must be a no-op");

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
        1,
        "no duplicate node_completed"
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStarted, "consumer"),
        1,
        "no downstream re-spawn"
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeWaiting, "consumer"),
        0
    );
}

/// A duplicate completion on a **Completed** Run is a no-op that leaves the Run
/// as it was: the embedded re-open (ADR-0049) is for a completion that changes
/// something, so it must not fire before the duplicate is recognised. It used
/// to: the Run was re-opened, then the completion was ignored, and the Run sat
/// `running` with every node done.
#[tokio::test]
async fn a_duplicate_completion_on_a_completed_run_does_not_reopen_it() {
    for kind in ["mark_node_done", "skip_node"] {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "dup-on-completed";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "dup-on-completed-1";
        for (kind, node) in [
            (event_log::EventKind::RunStarted, None),
            (event_log::EventKind::NodeStarted, Some("worker")),
            (event_log::EventKind::NodeCompleted, Some("worker")),
            (event_log::EventKind::NodeStarted, Some("consumer")),
            (event_log::EventKind::NodeCompleted, Some("consumer")),
            (event_log::EventKind::RunCompleted, None),
        ] {
            append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: run_id.into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: kind.clone(),
                    node_id: node.map(Into::into),
                    iter: node.map(|_| 1),
                    payload: (kind == event_log::EventKind::RunStarted)
                        .then(|| serde_json::json!({ "pipeline_name": pipe_name })),
                },
            )
            .await
            .unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"kind": "{kind}", "node_id": "worker", "iter": 1}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{kind}");
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["noop"], true, "{kind}: a duplicate is a no-op");

        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(
            !events
                .iter()
                .any(|e| e.kind == event_log::EventKind::CommandIssued
                    && e.payload.as_ref().and_then(|p| p.get("command"))
                        == Some(&serde_json::json!("reopen_run"))),
            "{kind}: a duplicate must not re-open the Run"
        );
        assert_eq!(
            event_log::project(&events).unwrap().status,
            event_log::RunStatus::Completed,
            "{kind}: the Run stays Completed"
        );
    }
}

#[tokio::test]
async fn mark_node_done_on_never_started_iter_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-ghost-mark";
    write_pipeline_with_consumer(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "guard-ghost-mark-1";
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
    };
    append_event(&state, &run_started).await.unwrap();

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"], "completion_rejected");
    assert_eq!(body["recoverable"], false);
    assert!(
        body["message"].as_str().unwrap().contains("never started"),
        "got {body}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
        0,
        "rejection emits no completion"
    );
}

#[tokio::test]
async fn failed_run_accepts_no_completion_and_schedules_nothing() {
    // Once the run is failed, node_done is rejected and the
    // scheduler does not advance until resume_run.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-failed-sched";
    write_pipeline_with_consumer(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "guard-failed-sched-1";
    for event in [
        seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": pipe_name })),
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::RunFailed,
            None,
            None,
            Some(serde_json::json!({ "reason": "boom" })),
        ),
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iter": 1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
        0
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStarted, "consumer")
            + count_events(&events, event_log::EventKind::NodeWaiting, "consumer"),
        0,
        "a failed run schedules nothing"
    );
}

#[tokio::test]
async fn double_node_done_is_noop_without_downstream_respawn() {
    // Replaying node_done for an already-completed (node, iter) must
    // not bump the consumer to a fresh iteration.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-dup-done";
    write_pipeline_with_consumer(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "guard-dup-done-1";
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("consumer".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iter": 1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
        1,
        "no duplicate node_completed"
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStarted, "consumer"),
        1,
        "no downstream re-spawn"
    );
}

#[tokio::test]
async fn node_done_returns_409_when_outputs_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "node-done-validate";
    write_pipeline_with_outputs(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "node-done-409";

    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"], "missing_outputs");
    let missing = body["missing"].as_array().unwrap();
    assert!(missing.iter().any(|v| v == "summary"));
    assert!(missing.iter().any(|v| v == "report"));
}

/// A `worker` with an ordinary `report` and a declared live artifact `preview`
/// (ADR-0089) whose schema nothing enforces.
fn write_pipeline_with_live_artifact(dir: &Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: report\n    live_artifacts:\n      - name: preview\n        frontmatter:\n          verdict:\n            type: enum\n            allowed: [PASS, FAIL]\nedges: []\n"
    );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

async fn seed_running_worker(state: &Arc<TestState>, run_id: &str, pipe_name: &str) {
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(state, &event).await.unwrap();
    }
}

async fn post_done(state: Arc<TestState>, run_id: &str) -> (StatusCode, String) {
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/done"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    (status, body_string(resp).await)
}

/// A live artifact is never checked: absent, the completion goes through as
/// long as the outputs are there.
#[tokio::test]
async fn node_done_completes_with_an_absent_live_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "node-done-intermediate";
    write_pipeline_with_live_artifact(tmp.path(), pipe_name);
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "node-done-intermediate-absent";
    seed_running_worker(&state, run_id, pipe_name).await;
    write_artifact_for(tmp.path(), run_id, "worker", "report", "done");

    let (status, body) = post_done(state.clone(), run_id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = load_events(&state.db, run_id).await.unwrap();
    let projected = event_log::project(&events).unwrap();
    assert_eq!(
        projected.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
}

/// A frontmatter that breaks the schema of a live artifact is no mismatch, and
/// the refusal for a missing output names it alone.
#[tokio::test]
async fn node_done_refuses_on_the_ordinary_output_only() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "node-done-intermediate-refusal";
    write_pipeline_with_live_artifact(tmp.path(), pipe_name);
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "node-done-intermediate-refusal";
    seed_running_worker(&state, run_id, pipe_name).await;
    let live = worktree_dir_for_run(tmp.path(), run_id).join(".pdo/artifacts/worker/iter-1/live");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("preview.md"), "---\nverdict: MAYBE\n---\n").unwrap();

    let (status, body) = post_done(state.clone(), run_id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["error"], "missing_outputs");
    assert_eq!(body["missing"], serde_json::json!(["report"]));

    write_artifact_for(tmp.path(), run_id, "worker", "report", "done");
    let (status, body) = post_done(state, run_id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A `script` node whose declared output never landed. The whole point of #490:
/// this used to answer `200` **after** appending `NodeFailed` + `RunFailed`, so
/// `pdo complete` printed "marked complete." and exited `0` on a dead run.
#[tokio::test]
async fn script_validation_failed_is_409_not_200() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe = "script-refusal";
    write_script_pipeline_with_outputs(tmp.path(), pipe);
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "script-refusal-run";
    seed_awaiting_run(&state, run_id, pipe).await;

    let (status, ct, body) = done_triplet(&state, run_id, "worker", "{}").await;
    assert_eq!((status, ct.as_str()), (StatusCode::CONFLICT, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["error"], "script_validation_failed");
    assert_eq!(body["recoverable"], false);
    // The detail stays NESTED (ADR-0035 §5) so a fail-fast audit trail is
    // distinguishable from an after-retry one.
    assert_eq!(body["detail"]["kind"], "missing_outputs");
    assert_eq!(body["detail"]["missing"], serde_json::json!(["out"]));

    // ADR-0049: the node is interrupted (an output miss is a give-up, not a
    // failure) and the run parks AwaitingUser — never RunFailed. The refusal
    // is still non-recoverable (the script agent is gone), so exit code 4
    // stays meaningful.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on an output-validation miss (ADR-0049)"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == event_log::EventKind::NodeInterrupted)
            .count(),
        1
    );

    // The projection still carries the missing port, so the red banner is no
    // longer a list of nothing.
    let projected = event_log::project(&events).unwrap();
    assert_eq!(projected.nodes["worker"].missing_outputs, vec!["out"]);
    assert_eq!(projected.status, event_log::RunStatus::AwaitingUser);
}

/// The first frontmatter mismatch, on **both** routes. Nothing terminal is
/// appended, the node stays `running`, and `recoverable: true` is what tells the
/// caller to fix and retry rather than to signal a failure.
#[tokio::test]
async fn frontmatter_retry_pending_is_409_and_recoverable_on_both_routes() {
    for (label, use_done_route) in [("done", true), ("commands", false)] {
        let tmp = tempfile::tempdir().unwrap();
        let pipe = "fm-pending";
        write_pipeline_with_frontmatter(tmp.path(), pipe);
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "fm-pending-run";
        seed_awaiting_run(&state, run_id, pipe).await;
        write_artifact_for(
            tmp.path(),
            run_id,
            "worker",
            "review",
            "---\nverdict: MAYBE\n---\nnope\n",
        );

        let (status, ct, raw) = if use_done_route {
            done_triplet(&state, run_id, "worker", "{}").await
        } else {
            mark_done_triplet(&state, run_id, "worker").await
        };
        assert_eq!(
            (status, ct.as_str()),
            (StatusCode::CONFLICT, JSON_CT),
            "{label}"
        );
        let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(body["error"], "frontmatter_retry_pending", "{label}");
        assert_eq!(body["recoverable"], true, "{label}");
        assert_eq!(body["violations"][0]["field"], "verdict", "{label}");

        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(
            !events.iter().any(|e| matches!(
                e.kind,
                event_log::EventKind::NodeFailed | event_log::EventKind::RunFailed
            )),
            "{label}: a recoverable refusal must append nothing terminal"
        );
        // Still alive — `awaiting_user` here because the seed parks the node
        // there; what matters is that it is not terminal.
        let projected = event_log::project(&events).unwrap();
        assert!(
            !matches!(
                projected.nodes["worker"].status,
                event_log::NodeStatus::Failed | event_log::NodeStatus::Completed
            ),
            "{label}: a recoverable refusal must leave the node alive"
        );
    }
}

/// The retry is spent: `recoverable: false`, and `NodeFailed` + `RunFailed` are
/// already in the log. This is the arm that answered `200` on a dead run.
#[tokio::test]
async fn frontmatter_retry_exhausted_is_409_and_not_recoverable() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe = "fm-exhausted";
    write_pipeline_with_frontmatter(tmp.path(), pipe);
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "fm-exhausted-run";
    seed_awaiting_run(&state, run_id, pipe).await;
    write_artifact_for(
        tmp.path(),
        run_id,
        "worker",
        "review",
        "---\nverdict: MAYBE\n---\nnope\n",
    );

    // First attempt spends the retry (`FrontmatterRetryPending`).
    let (first, _, _) = mark_done_triplet(&state, run_id, "worker").await;
    assert_eq!(first, StatusCode::CONFLICT);

    let (status, ct, raw) = mark_done_triplet(&state, run_id, "worker").await;
    assert_eq!((status, ct.as_str()), (StatusCode::CONFLICT, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(body["error"], "frontmatter_retry_exhausted");
    assert_eq!(body["recoverable"], false);

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a validation miss (ADR-0049)"
    );
    let projected = event_log::project(&events).unwrap();
    // ADR-0049: an exhausted validation retry interrupts the node (parking the
    // run AwaitingUser), it never fails it.
    assert_eq!(
        projected.nodes["worker"].status,
        event_log::NodeStatus::Interrupted
    );
    assert_eq!(projected.status, event_log::RunStatus::AwaitingUser);
    assert!(projected.nodes["worker"]
        .failure_reason
        .as_deref()
        .unwrap_or_default()
        .contains("output validation after retry"));
}

/// #654 / ADR-0060: the inverse of the pre-#654 test that lived here. A
/// non-isolated NodeRun that modifies a TRACKED file is no longer refused —
/// PDO commits its work onto the Run's branch and the completion goes
/// through, on `POST …/done` and on the `mark_node_done` command arm alike.
#[tokio::test]
async fn a_shared_worktree_node_that_dirties_a_tracked_file_is_delivered_not_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_git_repo_for_refusal_tests(repo);
    let pipe = "shared-delivery";
    write_pipeline_with_outputs(repo, pipe);

    let state = test_state_with_dir(repo).await;
    let run_id = "shared-delivery-run";
    let wt_dir = worktree_dir_for_run(repo, run_id);
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    // A tracked file modified AND a brand-new untracked one: both are the
    // node's work, and `git add -A` takes both.
    std::fs::write(
        wt_dir.join("tracked.txt"),
        "mutated by a non-isolated node\n",
    )
    .unwrap();
    std::fs::write(wt_dir.join("brand_new.rs"), "pub fn added() {}\n").unwrap();
    assert!(worktree_has_tracked_changes(&wt_dir).unwrap());
    write_artifact_for(repo, run_id, "worker", "summary", "s\n");
    write_artifact_for(repo, run_id, "worker", "report", "r\n");

    // The isolation is read from the PROJECTED node defs, so `RunStarted` has
    // to carry them.
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": pipe,
                "node_defs": [
                    { "id": "worker", "node_type": "agent", "isolated_worktree": false, "inputs": [], "outputs": [] }
                ],
                "edges": [],
            })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let (status, _, _) = done_triplet(&state, run_id, "worker", "{}").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a non-isolated node that edited files must complete, not be refused"
    );

    // The commit landed on the Run's branch, under the deterministic message.
    let log = std::process::Command::new("git")
        .args(["log", "--format=%s", "-1", &pipeline_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "worker iter-1: completed"
    );
    // …and it carries BOTH the tracked edit and the new file.
    let files = std::process::Command::new("git")
        .args(["show", "--name-only", "--format=", &pipeline_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&files.stdout);
    assert!(
        files.contains("tracked.txt") && files.contains("brand_new.rs"),
        "the delivery commits every non-ignored change; got: {files}"
    );

    // The delivery is recorded, before the terminal event, with both tips.
    let events = load_events(&state.db, run_id).await.unwrap();
    let delivered_at = events
        .iter()
        .position(|e| e.kind == event_log::EventKind::NodeDelivered)
        .expect("a delivery that moved the branch records NodeDelivered");
    let completed_at = events
        .iter()
        .position(|e| e.kind == event_log::EventKind::NodeCompleted)
        .expect("the node completed");
    assert!(
        delivered_at < completed_at,
        "the delivery must precede the terminal event"
    );
    let projected = event_log::project(&events).unwrap();
    let delivery = projected.nodes["worker"]
        .delivery
        .as_ref()
        .expect("the projection carries the delivery");
    assert_ne!(delivery.before, delivery.after);
    assert!(!events
        .iter()
        .any(|e| e.kind == event_log::EventKind::NodeInterrupted));
}

/// A completion refused on its outputs leaves no trace: the node's edits are
/// neither committed nor recorded as delivered, on `POST …/done` and on the
/// `mark_node_done` command arm alike. Output validation reads the artifact
/// store, which never travels through git, so it runs before the delivery.
#[tokio::test]
async fn a_completion_refused_on_missing_outputs_delivers_nothing() {
    for (surface, mark_done) in [("done", false), ("mark_node_done", true)] {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_git_repo_for_refusal_tests(repo);
        let pipe = "refused-no-delivery";
        write_pipeline_with_outputs(repo, pipe);

        let state = test_state_with_dir(repo).await;
        let run_id = "refused-no-delivery-run";
        let wt_dir = worktree_dir_for_run(repo, run_id);
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
        let tip = |repo: &Path| {
            let out = std::process::Command::new("git")
                .args(["rev-parse", &pipeline_branch])
                .current_dir(repo)
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let tip_before = tip(repo);
        std::fs::write(wt_dir.join("tracked.txt"), "edited, outputs not written\n").unwrap();

        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": pipe,
                    "node_defs": [
                        { "id": "worker", "node_type": "agent", "isolated_worktree": false, "inputs": [], "outputs": [] }
                    ],
                    "edges": [],
                })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let (status, _, body) = if mark_done {
            command_triplet(
                &state,
                run_id,
                r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
            )
            .await
        } else {
            done_triplet(&state, run_id, "worker", "{}").await
        };
        assert_eq!(status, StatusCode::CONFLICT, "{surface}: {body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["error"], "missing_outputs", "{surface}");

        assert_eq!(
            tip(repo),
            tip_before,
            "{surface}: a refused completion must not commit the node's work"
        );
        assert!(
            worktree_has_tracked_changes(&wt_dir).unwrap(),
            "{surface}: the node's edit stays in the worktree, uncommitted"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(
            !events
                .iter()
                .any(|e| e.kind == event_log::EventKind::NodeDelivered),
            "{surface}: a refused completion must not record a delivery"
        );
    }
}

/// #654: a NodeRun that left nothing takes NO commit — the Run's branch does
/// not move and no `NodeDelivered` is recorded.
#[tokio::test]
async fn a_node_that_left_nothing_takes_no_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_git_repo_for_refusal_tests(repo);
    // No declared output port: this test is about the delivery, not about
    // output validation.
    let pipe = "shared-noop";
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(
            pipelines_dir.join(format!("{pipe}.yaml")),
            format!(
                "name: {pipe}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\nedges: []\n"
            ),
        )
        .unwrap();

    let state = test_state_with_dir(repo).await;
    let run_id = "shared-noop-run";
    let wt_dir = worktree_dir_for_run(repo, run_id);
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let tip_before = std::process::Command::new("git")
        .args(["rev-parse", &pipeline_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    let tip_before = String::from_utf8_lossy(&tip_before.stdout)
        .trim()
        .to_string();

    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": pipe,
                "node_defs": [
                    { "id": "worker", "node_type": "agent", "isolated_worktree": false, "inputs": [], "outputs": [] }
                ],
                "edges": [],
            })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let (status, _, _) = done_triplet(&state, run_id, "worker", "{}").await;
    assert_eq!(status, StatusCode::OK);

    let tip_after = std::process::Command::new("git")
        .args(["rev-parse", &pipeline_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&tip_after.stdout).trim(),
        tip_before,
        "no change ⇒ no commit, empty or otherwise"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeDelivered),
        "a no-op delivery records nothing"
    );
}

/// New coverage: no test reached the merge-conflict arm before #490 either. Two
/// sub-worktrees touching the same file, merged in order — the second conflicts.
#[tokio::test]
async fn merge_conflict_is_409_not_200() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_git_repo_for_refusal_tests(repo);
    let pipe = "merge-conflict-refusal";
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(
            pipelines_dir.join(format!("{pipe}.yaml")),
            format!(
                "name: {pipe}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: impl_a\n    name: impl_a\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: task\n    outputs:\n      - name: patch\n  - id: impl_b\n    name: impl_b\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: task\n    outputs:\n      - name: patch\n"
            ),
        )
        .unwrap();

    let state = test_state_with_dir(repo).await;
    let run_id = "merge-conflict-run";
    let wt_dir = worktree_dir_for_run(repo, run_id);
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let mut spawn_bases = std::collections::HashMap::new();
    for node in ["impl_a", "impl_b"] {
        let sub_dir = sub_worktree_path(repo, run_id, node, 1);
        let sub_branch = sub_worktree_branch(run_id, node, 1);
        // Both siblings were cut from the same tip. impl_a's merge moves
        // it, so impl_b's base goes stale — which is what makes its conflict a
        // *genuine* one the resolution must not swallow (AC2).
        spawn_bases.insert(
            node,
            create_sub_worktree(repo, &sub_dir, &sub_branch, &pipeline_branch).unwrap(),
        );
        std::fs::write(sub_dir.join("shared.txt"), format!("from {node}\n")).unwrap();
        // Both nodes declare a `patch` output; write it so validation is not
        // what refuses.
        write_artifact_for(repo, run_id, node, "patch", "done\n");
    }

    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": pipe,
                "node_defs": [
                    { "id": "impl_a", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] },
                    { "id": "impl_b", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] },
                ],
                "edges": [],
            })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("impl_a".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "base_sha": spawn_bases["impl_a"] })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("impl_b".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "base_sha": spawn_bases["impl_b"] })),
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    // First merge lands.
    let (first, _, _) = done_triplet(&state, run_id, "impl_a", "{}").await;
    assert!(first.is_success(), "first merge should land, got {first}");

    // Second conflicts — and this is the refusal that used to answer `200`
    // *after* appending `MergeConflictDetected` + `RunFailed`.
    let (status, ct, raw) = done_triplet(&state, run_id, "impl_b", "{}").await;
    assert_eq!((status, ct.as_str()), (StatusCode::CONFLICT, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(body["error"], "merge_conflict");
    assert_eq!(body["recoverable"], false);

    let events = load_events(&state.db, run_id).await.unwrap();
    let conflict = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::MergeConflictDetected)
        .expect("a merge_conflict_detected event");

    // #503 AC3/AC4: the event has to be diagnosable on its own. `detail` was
    // `git merge`'s stderr, which is byte-empty on a conflict, so every one of
    // these events ever written said `detail: ""` about N conflicting files.
    let payload = conflict.payload.as_ref().unwrap();
    assert!(
        payload["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("CONFLICT"),
        "detail must carry git's report; got {:?}",
        payload["detail"]
    );
    assert_eq!(
        payload["conflicting_files"],
        serde_json::json!(["shared.txt"])
    );
    for tip in ["pipeline_tip", "node_tip"] {
        assert_eq!(
            payload[tip].as_str().unwrap_or_default().len(),
            40,
            "{tip} must be a full SHA; got {:?}",
            payload[tip]
        );
    }

    // #503 AC5 / ADR-0049: the node must not stay projected `running`, but a
    // merge conflict is a runtime give-up → the node is `Interrupted` and the
    // run parks `AwaitingUser`, never `Failed`.
    let projected = event_log::project(&events).unwrap();
    assert_eq!(projected.status, event_log::RunStatus::AwaitingUser);
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a merge conflict (ADR-0049)"
    );
    assert_eq!(
        projected.nodes["impl_b"].status,
        event_log::NodeStatus::Interrupted,
        "the node whose merge-back conflicted must be Interrupted, not Running/Failed"
    );
    // …and the Run must say why, in the awaiting-reason field a UI can render.
    let run_reason = projected.awaiting_reason.expect("an awaiting reason");
    assert!(
        run_reason.contains("merge conflict on impl_b") && run_reason.contains("1 conflicting"),
        "the run's reason must name the node and the blast radius; got {run_reason:?}"
    );
    assert_eq!(
        projected.nodes["impl_b"].failure_reason.as_deref(),
        Some(run_reason.as_str()),
        "node and run must agree on the cause"
    );
}

/// #503 / ADR-0036: the terminal-node rebase. A single-node Run whose node
/// rewrote its branch onto a base that moved mid-Run must **complete**: the
/// pipeline branch is no longer an ancestor of the node's branch, the three-way
/// merge conflicts, and resolving it in the node's favour drops nothing because
/// nothing else reached the pipeline branch since the node was cut from it.
///
/// The pre-#503 daemon answered `409 merge_conflict` here and filed the Run
/// `failed` with its work already delivered.
#[tokio::test]
async fn a_terminal_node_that_rebased_still_completes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_git_repo_for_refusal_tests(repo);
    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let base_branch = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    let pipe = "terminal-rebase";
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(
            pipelines_dir.join(format!("{pipe}.yaml")),
            format!(
                "name: {pipe}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: ship\n    name: ship\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: task\n    outputs:\n      - name: patch\n"
            ),
        )
        .unwrap();

    let state = test_state_with_dir(repo).await;
    let run_id = "terminal-rebase-run";
    let wt_dir = worktree_dir_for_run(repo, run_id);
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    // An earlier node's work, already merged back onto the pipeline branch.
    std::fs::write(wt_dir.join("VERSION"), "1.5.0\n").unwrap();
    git(&wt_dir, &["add", "-A"]);
    git(&wt_dir, &["commit", "-m", "implementer: 1.5.0"]);

    // A concurrent Run lands on the integration branch while this Run is live.
    std::fs::write(repo.join("VERSION"), "1.5.0\n").unwrap();
    std::fs::write(repo.join("concurrent.rs"), "pub fn other() {}\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "concurrent run: 1.5.0"]);

    // The terminal node, cut from the pipeline branch, rebases onto it and bumps.
    let sub_dir = sub_worktree_path(repo, run_id, "ship", 1);
    let sub_branch = sub_worktree_branch(run_id, "ship", 1);
    let spawn_base = create_sub_worktree(repo, &sub_dir, &sub_branch, &pipeline_branch).unwrap();
    let rebase = std::process::Command::new("git")
        .args(["rebase", &base_branch])
        .current_dir(&sub_dir)
        .output()
        .unwrap();
    if !rebase.status.success() {
        std::fs::write(sub_dir.join("VERSION"), "1.5.0\n").unwrap();
        git(&sub_dir, &["add", "-A"]);
        let cont = std::process::Command::new("git")
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .current_dir(&sub_dir)
            .output()
            .unwrap();
        assert!(cont.status.success(), "rebase --continue failed");
    }
    std::fs::write(sub_dir.join("VERSION"), "1.6.0\n").unwrap();
    git(&sub_dir, &["add", "-A"]);
    git(&sub_dir, &["commit", "-m", "ship it: 1.6.0"]);
    write_artifact_for(repo, run_id, "ship", "patch", "done\n");

    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": pipe,
                "node_defs": [
                    { "id": "ship", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] },
                ],
                "edges": [],
            })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("ship".into()),
            iter: Some(1),
            // What a real spawn records: the commit the sub-worktree was
            // cut from. It is the *only* thing that authorises the resolution.
            payload: Some(serde_json::json!({ "base_sha": spawn_base })),
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let (status, _, raw) = done_triplet(&state, run_id, "ship", "{}").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a delivered Run must not be refused over PDO's own bookkeeping; got {raw}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    let resolved = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::MergeResolvedInNodeFavour)
        .expect("the rewrite must be logged, never silent");
    let payload = resolved.payload.as_ref().unwrap();
    assert_eq!(payload["conflicting_files"], serde_json::json!(["VERSION"]));
    assert_eq!(
        payload["pipeline_tip"].as_str(),
        Some(spawn_base.as_str()),
        "the rewritten tip must be the base the node was cut from — that IS the rule"
    );
    assert!(payload["rule"]
        .as_str()
        .unwrap()
        .contains("still the commit"));
    assert_eq!(
        payload["merge_commit"].as_str().map(str::len),
        Some(40),
        "the resolution commit must be named, so the rewrite is auditable"
    );
    assert!(!events
        .iter()
        .any(|e| e.kind == event_log::EventKind::RunFailed));

    // The pipeline branch carries the node's tree — including what the node
    // rebased onto — so a downstream node would see what was delivered.
    assert_eq!(
        std::fs::read_to_string(wt_dir.join("VERSION")).unwrap(),
        "1.6.0\n"
    );
    assert!(wt_dir.join("concurrent.rs").exists());
    let projected = event_log::project(&events).unwrap();
    assert_eq!(
        projected.nodes["ship"].status,
        event_log::NodeStatus::Completed
    );
}

/// A legal duplicate is a **success**: `200`, `noop: true`, exit `0`. Guarded on
/// both surfaces because an agent that read "refused" here and chained
/// `pdo fail` would kill a run that had just succeeded — and a `script` node's
/// tail would do it unprompted.
#[tokio::test]
async fn a_legal_duplicate_completion_is_still_a_200_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe = "noop-dup";
    write_test_pipeline(tmp.path(), pipe);
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "noop-dup-run";
    seed_awaiting_run(&state, run_id, pipe).await;
    write_artifact_for(tmp.path(), run_id, "worker", "result", "done\n");

    let (first, _, _) = done_triplet(&state, run_id, "worker", "{}").await;
    assert!(first.is_success(), "first completion should land: {first}");

    for (label, raw) in [
        ("done", done_triplet(&state, run_id, "worker", "{}").await),
        (
            "commands",
            mark_done_triplet(&state, run_id, "worker").await,
        ),
    ] {
        let (status, ct, raw) = raw;
        assert_eq!(
            (status, ct.as_str()),
            (StatusCode::OK, JSON_CT),
            "{label}: a legal duplicate must stay a 2xx"
        );
        let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(body["noop"], true, "{label}");
        assert!(body["reason"].is_string(), "{label}");
    }
}
