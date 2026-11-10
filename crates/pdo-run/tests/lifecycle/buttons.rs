//! The canvas buttons on a node: Stop, Retry and what a retry would invalidate.

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
async fn node_stop_returns_404_for_unknown_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/nodes/worker/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_stop_returns_conflict_when_not_running() {
    let state = test_state().await;
    seed_run_for_node_control(&state, "stop-notrun", "test-pipe").await;
    seed_node_started(&state, "stop-notrun", "worker", 1).await;
    seed_node_completed(&state, "stop-notrun", "worker", 1).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/stop-notrun/nodes/worker/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn node_stop_returns_not_found_for_unknown_node() {
    let state = test_state().await;
    seed_run_for_node_control(&state, "stop-nonode", "test-pipe").await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/stop-nonode/nodes/ghost/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_stop_emits_stopped_event() {
    let state = test_state().await;
    seed_run_for_node_control(&state, "stop-ok", "test-pipe").await;
    seed_node_started(&state, "stop-ok", "worker", 1).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/stop-ok/nodes/worker/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "stop-ok").await.unwrap();
    let stopped = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::NodeStopped);
    assert!(stopped.is_some(), "should have NodeStopped event");
    let stopped = stopped.unwrap();
    assert_eq!(stopped.node_id.as_deref(), Some("worker"));
    assert_eq!(stopped.iter, Some(1));

    let run_state = event_log::project(&events).unwrap();
    assert_eq!(
        run_state.nodes.get("worker").unwrap().status,
        event_log::NodeStatus::Stopped
    );
    assert_ne!(
        run_state.status,
        event_log::RunStatus::Failed,
        "NodeStopped must NOT transition the run to failed"
    );
}

#[tokio::test]
async fn node_stop_idempotent_second_call_returns_conflict() {
    let state = test_state().await;
    seed_run_for_node_control(&state, "stop-idem", "test-pipe").await;
    seed_node_started(&state, "stop-idem", "worker", 1).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/stop-idem/nodes/worker/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let app2 = build_router(state);
    let resp2 = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/stop-idem/nodes/worker/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn node_retry_returns_404_for_unknown_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/nodes/worker/retry")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_retry_invalidates_downstream_and_returns_list() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "retry-downstream";
    let pipeline_yaml = "\
name: test-pipe
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
      - name: code
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
";

    let pipelines_dir = repo_root.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

    let worktree_dir = worktree_dir_for_run(repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let reviewer_artifacts = artifacts_dir.join("reviewer").join("iter-1").join("review");
    std::fs::create_dir_all(&reviewer_artifacts).unwrap();
    std::fs::write(reviewer_artifacts.join("output.md"), "old review").unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;
    seed_node_completed(&state, run_id, "worker", 1).await;
    seed_node_started(&state, run_id, "reviewer", 1).await;
    seed_node_completed(&state, run_id, "reviewer", 1).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["iter"], 2);

    let invalidated = json["invalidated"]
        .as_array()
        .expect("invalidated should be an array");
    assert!(
        invalidated.iter().any(|v| v == "reviewer"),
        "reviewer should be in invalidated list, got: {invalidated:?}"
    );

    assert!(
        !reviewer_artifacts.exists(),
        "reviewer artifacts should have been deleted"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();

    assert!(
        !run_state.nodes.contains_key("reviewer"),
        "reviewer should be removed from state (pending) after invalidation"
    );

    let has_invalidated_worker = events.iter().any(|e| {
        e.kind == event_log::EventKind::NodeInvalidated && e.node_id.as_deref() == Some("worker")
    });
    assert!(
        has_invalidated_worker,
        "should have NodeInvalidated event for worker (self-invalidation)"
    );

    let has_invalidated_reviewer = events.iter().any(|e| {
        e.kind == event_log::EventKind::NodeInvalidated && e.node_id.as_deref() == Some("reviewer")
    });
    assert!(
        has_invalidated_reviewer,
        "should have NodeInvalidated event for reviewer (downstream)"
    );
}

#[tokio::test]
async fn node_retry_stops_running_node_first() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "retry-stop-first";
    let pipeline_yaml = "\
name: test-pipe
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
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";
    let pipelines_dir = repo_root.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

    let worktree_dir = worktree_dir_for_run(repo_root, run_id);
    std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    let has_stopped = events
        .iter()
        .any(|e| e.kind == event_log::EventKind::NodeStopped);
    assert!(has_stopped, "should have NodeStopped event before retry");
}

#[tokio::test]
async fn node_retry_idempotent_on_pending_node() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "retry-pending";
    let pipeline_yaml = "\
name: test-pipe
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
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";
    let pipelines_dir = repo_root.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

    let worktree_dir = worktree_dir_for_run(repo_root, run_id);
    std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    // Worker has never started — it's implicitly pending (not in run state)

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["iter"], 2);
}

#[tokio::test]
async fn node_retry_on_terminal_run_embeds_reopen() {
    // AC7 / ADR-0049 (amends #487): Play/Retry on a node of a terminal (here
    // `Failed`) Run now EMBEDS the re-open — the human's own gesture re-opens
    // the Run atomically and re-drives the node, instead of the pre-résilience
    // 409 "resume the run first". A node button still never re-opens of the
    // runtime's own initiative (ADR-0009 amended); the human's retry is the
    // re-open.
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    write_retry_pipeline(repo_root);
    let state = test_state_with_dir(repo_root).await;
    let run_id = "retry-terminal";
    std::fs::create_dir_all(
        worktree_dir_for_run(repo_root, run_id)
            .join(".pdo")
            .join("artifacts"),
    )
    .unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;
    seed_run_terminal(&state, run_id, event_log::EventKind::RunFailed).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // A 2xx — the retry was accepted, not refused.
    assert!(
        resp.status().is_success(),
        "retry must embed the re-open and succeed, got {}",
        resp.status()
    );

    // The re-open gesture is in the log, the Run is live again, and the node
    // re-spawned at iter 2. The terminal `RunFailed` label is preserved.
    let after = load_events(&state.db, run_id).await.unwrap();
    assert!(
        after
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued
                && e.payload.as_ref().and_then(|p| p.get("command"))
                    == Some(&serde_json::json!("reopen_run"))),
        "the retry must embed a reopen_run CommandIssued"
    );
    assert!(
        after
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the previous terminal label stays in the event log (AC12)"
    );
    assert!(
        after
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted && e.iter == Some(2)),
        "the retry re-spawned the node at iter 2 after the embedded re-open"
    );
    let projected = event_log::project(&after).unwrap();
    assert!(
        projected.status.is_live(),
        "the Run is live again after the embedded re-open, got {:?}",
        projected.status
    );
}

#[tokio::test]
async fn node_retry_on_live_run_with_failed_node_spawns_truthfully() {
    // Non-regression: retry of a `failed` node on a still-live Run is
    // unchanged in spirit — it re-spawns at iter+1 — and now reports a TRUTHFUL
    // Spawned verdict (the historical `{iter, invalidated}` root plus `spawned`).
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    write_retry_pipeline(repo_root);
    let state = test_state_with_dir(repo_root).await;
    let run_id = "retry-failed-live";
    std::fs::create_dir_all(
        worktree_dir_for_run(repo_root, run_id)
            .join(".pdo")
            .join("artifacts"),
    )
    .unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;
    // Fail the iteration, then resume the Run so it stays live with a failed
    // node — exactly the state the operator retries from (session died, #485).
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::NodeFailed,
            Some("worker"),
            Some(1),
            None,
        ),
    )
    .await
    .unwrap();
    append_event(
        &state,
        &seed_event(run_id, event_log::EventKind::RunResumed, None, None, None),
    )
    .await
    .unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["iter"], 2, "retry advances to iter+1");
    assert_eq!(
        json["spawned"][0]["node_id"], "worker",
        "the truthful verdict names what was spawned"
    );

    // The reservation is durable: a NodeStarted for iter 2 is on the log.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")
                && e.iter == Some(2)),
        "a spawned retry must append NodeStarted for the new iteration"
    );
}

#[tokio::test]
async fn node_retry_preview_returns_downstream_info() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "preview-test";
    let pipeline_yaml = "\
name: test-pipe
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
      - name: code
  - id: reviewer
    name: Reviewer
    type: agent
    isolated_worktree: false
    inputs:
      - name: code
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
";

    let pipelines_dir = repo_root.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

    let worktree_dir = worktree_dir_for_run(repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let reviewer_artifacts = artifacts_dir.join("reviewer").join("iter-1");
    std::fs::create_dir_all(&reviewer_artifacts).unwrap();
    std::fs::write(reviewer_artifacts.join("review.md"), "old review").unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/runs/{run_id}/nodes/worker/retry/preview"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let downstream = json["downstream"].as_array().unwrap();
    assert!(
        downstream.iter().any(|v| v == "reviewer"),
        "reviewer should be in downstream"
    );

    assert_eq!(
        json["affected_count"], 1,
        "reviewer has artifacts, so affected_count should be 1"
    );

    let with_artifacts = json["with_artifacts"].as_array().unwrap();
    assert!(
        with_artifacts.iter().any(|v| v == "reviewer"),
        "reviewer should be in with_artifacts"
    );
}

#[tokio::test]
async fn node_retry_preview_returns_404_for_unknown_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/runs/no-such-run/nodes/worker/retry/preview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_retry_preview_zero_affected_when_no_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "preview-no-artifacts";
    let pipeline_yaml = "\
name: test-pipe
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
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";

    let pipelines_dir = repo_root.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

    let worktree_dir = worktree_dir_for_run(repo_root, run_id);
    std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

    seed_run_for_node_control(&state, run_id, "test-pipe").await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/runs/{run_id}/nodes/worker/retry/preview"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["affected_count"], 0);
}
