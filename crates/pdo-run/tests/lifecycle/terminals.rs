//! The agent's `fail` and `skip`.

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
async fn node_fail_marks_run_failed() {
    let state = test_state().await;

    let run_id = "test-fail-run";
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

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-fail-run/nodes/worker/fail")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason": "something broke"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // ADR-0049 / AC5: with `auto_fail` unset (the default), an agent `pdo fail`
    // parks the run `AwaitingUser` for a human to confirm — NOT `Failed`. The
    // node itself is `Failed` (the agent deliberately failed its iteration).
    // The run-level park append lives in the detached tail — poll.
    let run_state = wait_run_status(&state, run_id, event_log::RunStatus::AwaitingUser).await;
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Failed
    );
    assert!(
        run_state.awaiting_reason.is_some(),
        "the park carries the fail reason as the awaiting reason"
    );
}

#[tokio::test]
async fn node_fail_with_run_auto_fail_terminalises_directly() {
    // AC5: with `auto_fail` opted in at the Run tier, an agent `pdo fail`
    // terminalises the run to `Failed` directly (no human confirmation).
    let state = test_state().await;
    let run_id = "test-autofail-run";
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test", "auto_fail": true })),
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

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/fail"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason": "something broke"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let run_state = wait_run_status(&state, run_id, event_log::RunStatus::Failed).await;
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Failed
    );
}

#[tokio::test]
async fn node_skip_returns_404_for_nonexistent_run() {
    // Pin the 404 BEFORE routing `node_skip` through `load_projected`.
    // The body is mandatory (`NodeSkipRequest.reason` has no default): without
    // it the response would be a 422 Json rejection, not the 404 under test.
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/nodes/selector/skip")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason": "no eligible issue"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_skip_marks_run_skipped_not_failed() {
    // A graceful no-op terminalizes the node as Completed and the run
    // as Skipped — never Failed, and downstream is short-circuited.
    let state = test_state().await;

    let run_id = "test-skip-run";
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
        node_id: Some("selector".into()),
        iter: Some(1),
        payload: None,
    };
    append_event(&state, &node_started).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-skip-run/nodes/selector/skip")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason": "no eligible issue"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // The RunSkipped append lives in the detached tail — poll.
    let run_state = wait_run_status(&state, run_id, event_log::RunStatus::Skipped).await;
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_ne!(run_state.status, event_log::RunStatus::Failed);
    // The selector itself is honestly terminal-Completed, not Failed.
    assert_eq!(
        run_state.nodes["selector"].status,
        event_log::NodeStatus::Completed
    );
    // No RunFailed event was ever appended.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "graceful no-op must not append a RunFailed event"
    );
}

#[tokio::test]
async fn node_skip_on_an_already_completed_node_is_an_honest_noop() {
    // #601 FP #6: a valid command with no effect answers `200 {noop, reason}`,
    // never a false success. Skipping a node whose iteration is already
    // terminal (Completed) while the run is still live routes through the same
    // completion head gate as `pdo complete`, which returns a legal-duplicate
    // NoOp — honest, and it appends nothing new.
    let state = test_state().await;
    let run_id = "skip-noop";
    for ev in [
        (
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "test" })),
        ),
        (
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
        (
            event_log::EventKind::NodeCompleted,
            Some("worker"),
            Some(1),
            None,
        ),
    ] {
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: ev.0,
                node_id: ev.1.map(String::from),
                iter: ev.2,
                payload: ev.3,
            },
        )
        .await
        .unwrap();
    }

    let before = load_events(&state.db, run_id).await.unwrap().len();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/skip-noop/nodes/worker/skip")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason":"already done"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        json["noop"],
        serde_json::json!(true),
        "must be an honest noop: {json}"
    );
    assert!(
        json["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "the noop must carry a reason: {json}"
    );
    // Honest = no new event appended (never a false success that writes state).
    let after = load_events(&state.db, run_id).await.unwrap().len();
    assert_eq!(before, after, "a noop must append nothing");
}

#[tokio::test]
async fn duplicate_node_skip_cannot_corrupt_a_terminal_run() {
    // The first skip terminalizes the run (Skipped). A second skip is then
    // rejected by the transition guard (409 — "run is Skipped, resume
    // first"), exactly like a duplicate `pdo complete` on a terminal run
    //. Crucially, it must NEVER double-append RunSkipped/NodeCompleted.
    let state = test_state().await;

    let run_id = "test-skip-idem";
    append_event(
        &state,
        &event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        },
    )
    .await
    .unwrap();
    append_event(
        &state,
        &event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("selector".into()),
            iter: Some(1),
            payload: None,
        },
    )
    .await
    .unwrap();

    let app = build_router(state.clone());
    let skip = || {
        app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-skip-idem/nodes/selector/skip")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"reason": "nothing to do"}"#))
                .unwrap(),
        )
    };

    assert_eq!(skip().await.unwrap().status(), StatusCode::OK);
    // The RunSkipped append lives in the detached tail: wait for the
    // run to actually turn terminal before probing the duplicate.
    wait_run_status(&state, run_id, event_log::RunStatus::Skipped).await;
    // Second skip: rejected because the run is already terminal (Skipped).
    assert_eq!(skip().await.unwrap().status(), StatusCode::CONFLICT);

    let events = load_events(&state.db, run_id).await.unwrap();
    let skips = events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::RunSkipped)
        .count();
    assert_eq!(skips, 1, "duplicate skip must not double-append RunSkipped");
    let completions = events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::NodeCompleted)
        .count();
    assert_eq!(
        completions, 1,
        "duplicate skip must not re-complete the node"
    );
    // And the run is still cleanly Skipped — not flipped to Failed.
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Skipped);
}
