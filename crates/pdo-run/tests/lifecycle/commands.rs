//! The Pipeline Manager's commands (`POST /runs/{id}/commands`), arm by arm.

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
async fn unknown_command_returns_bad_request() {
    let state = test_state().await;
    let run_id = "cmd-unknown";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "bogus_command"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reopen_run_lifts_a_terminal_run_and_preserves_the_label() {
    // AC6/AC8/FP#8: the global reopen_run command lifts a terminal (Completed
    // here) run back to Running, and the previous terminal label stays in the
    // event log. Completed nodes are never re-spawned (anti-#221).
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "test-pipe");
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reopen-completed";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::NodeCompleted,
            Some("worker"),
            Some(1),
            None,
        ),
    )
    .await
    .unwrap();
    seed_run_terminal(&state, run_id, event_log::EventKind::RunCompleted).await;

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "reopen_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    let state_after = event_log::project(&events).unwrap();
    assert_eq!(state_after.status, event_log::RunStatus::Running);
    // The terminal label is preserved (AC12).
    assert!(events
        .iter()
        .any(|e| e.kind == event_log::EventKind::RunCompleted));
    // The satisfied node stays Completed — never re-spawned (anti-#221).
    assert_eq!(
        state_after.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
}

#[tokio::test]
async fn reopen_run_refuses_a_live_run() {
    // reopen_run is for terminal / incident-parked runs; a cleanly Running run
    // needs no re-projection and is refused (not_reopenable).
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "test-pipe");
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reopen-live";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_node_started(&state, run_id, "worker", 1).await;

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "reopen_run"}"#))
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
    assert_eq!(body["error"], "not_reopenable");
}

#[tokio::test]
async fn restart_node_rejected_while_newer_iteration_is_live() {
    // restart_node on a stale iter must not race the scheduler's
    // newer live iteration — reject with a readable cause, spawn nothing.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-restart-race";
    write_pipeline_with_consumer(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "guard-restart-race-1";
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
            event_log::EventKind::NodeCompleted,
            Some("worker"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(2),
            None,
        ),
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
                    r#"{"kind": "restart_node", "node_id": "worker", "iter": 1}"#,
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
    // Assert the SLUG, never a substring of the prose (ADR-0037). The prose lives
    // in `message` now, and a substring assertion on `error` would have kept
    // passing BY COINCIDENCE — on a discriminating slug — while silently ceasing
    // to guard anything.
    assert_eq!(body["error"], "restart_refused", "got {body}");
    assert_eq!(body["recoverable"], true, "got {body}");
    // Nothing was touched: the guard is the FIRST probe, above the kill.
    assert_eq!(body["session_killed"], false, "got {body}");
    assert!(
        body["message"].as_str().unwrap().contains("still live"),
        "the guard's prose belongs in `message`; got {body}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    // A refused restart leaves NO trace at all — not even the audit event.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued),
        "a pre-kill refusal must append nothing: {events:?}"
    );
    // Iter 2 stays the only live iteration; iter 1 was not re-spawned.
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStarted, "worker"),
        2,
        "no extra node_started"
    );
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Running
    );
    assert_eq!(run_state.nodes["worker"].iter, 2);
}

#[tokio::test]
async fn resume_run_schedules_only_missing_work() {
    // Chain a -> b -> c with a, b completed and c never started.
    // resume_run schedules c only; a and b are not re-iterated.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-resume-missing";
    let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {pipe_name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: a\n    name: a\n    type: agent\n    isolated_worktree: false\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: b\n    name: b\n    type: agent\n    isolated_worktree: false\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: c\n    name: c\n    type: agent\n    isolated_worktree: false\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\nedges:\n  - source: {{ node: a, port: out }}\n    target: {{ node: b, port: in }}\n  - source: {{ node: b, port: out }}\n    target: {{ node: c, port: in }}\n"
        );
    std::fs::write(pipelines_dir.join(format!("{pipe_name}.yaml")), yaml).unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    saturate_session_cap(&state).await;
    let run_id = "guard-resume-missing-1";
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
            Some("a"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeCompleted,
            Some("a"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("b"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeCompleted,
            Some("b"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &event).await.unwrap();
    }

    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "resume_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, run_id).await.unwrap();
    // C is scheduled (waiting, because the cap is saturated)...
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeWaiting, "c")
            + count_events(&events, event_log::EventKind::NodeStarted, "c"),
        1,
        "the never-started node is scheduled exactly once"
    );
    // ...and the completed nodes are not re-iterated.
    for nid in ["a", "b"] {
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, nid),
            1,
            "completed node {nid} must not be re-spawned"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeWaiting, nid),
            0,
            "completed node {nid} must not be re-scheduled"
        );
    }
}

#[tokio::test]
async fn resume_run_refuses_concurrent_iteration_and_is_idempotent() {
    // A -> b with a back-edge b -> a (emergent cycle). a completed
    // iter 1, b still running iter 1: resume_run must NOT spawn b iter 2,
    // and a second resume_run must be a no-op too.
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "guard-resume-live";
    let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {pipe_name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: a\n    name: a\n    type: agent\n    isolated_worktree: false\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: b\n    name: b\n    type: agent\n    isolated_worktree: false\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\nedges:\n  - source: {{ node: a, port: out }}\n    target: {{ node: b, port: in }}\n  - source: {{ node: b, port: out }}\n    target: {{ node: a, port: in }}\n    when: \"iter < 3\"\n"
        );
    std::fs::write(pipelines_dir.join(format!("{pipe_name}.yaml")), yaml).unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    saturate_session_cap(&state).await;
    let run_id = "guard-resume-live-1";
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
            Some("a"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeCompleted,
            Some("a"),
            Some(1),
            None,
        ),
        seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("b"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &event).await.unwrap();
    }

    for _ in 0..2 {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "resume_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "b"),
            1,
            "no concurrent second iteration of b"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeWaiting, "b"),
            0,
            "no scheduled second iteration of b"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "a"),
            1,
            "completed a is not redone"
        );
    }
}

#[tokio::test]
async fn extend_cycle_requires_node_id() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "extend_cycle", "additional_iter": 3 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn extend_cycle_requires_positive_additional_iter() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "extend_cycle",
                        "node_id": "reviewer",
                        "additional_iter": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn end_region_requires_region_id() {
    // The manager routes a loop region BY ID (ADR-0011 / #152): without a
    // region_id the command is a bad request.
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "end_region" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn end_region_emits_command_issued_with_region_id() {
    // Ending a region by id appends a `command_issued` control-flow event
    // carrying the region id, so the projection can fold it.
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "region-end".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/region-end/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "end_region", "region_id": "review_loop" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "region-end").await.unwrap();
    let routes = event_log::collect_region_routes(&events);
    let route = routes
        .get("review_loop")
        .expect("end_region routed review_loop");
    assert!(route.ended, "the region is folded as ended");
}

#[tokio::test]
async fn bump_region_requires_positive_additional_iter() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "bump_region",
                        "region_id": "review_loop",
                        "additional_iter": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn bump_region_emits_command_issued_with_additional_iter() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "region-bump".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/region-bump/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "bump_region",
                        "region_id": "review_loop",
                        "additional_iter": 2
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "region-bump").await.unwrap();
    let routes = event_log::collect_region_routes(&events);
    let route = routes
        .get("review_loop")
        .expect("bump_region routed review_loop");
    assert_eq!(route.bumped_by, 2, "the region is folded with +2 laps");
    assert!(!route.ended);
}

#[tokio::test]
async fn end_region_resumes_an_exhausted_unrouted_halt() {
    // The acceptance behavior: a run blocked "exhausted — unrouted"
    // (a RunHalted) is continued by routing the region from the manager —
    // `resume_run` lifts the halt so the run is Running again, without a
    // daemon restart.
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "region-unstick".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let halt_event = event_log::Event {
        id: None,
        run_id: "region-unstick".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunHalted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "message": "exhausted — unrouted" })),
    };
    append_event(&state, &halt_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/region-unstick/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "end_region", "region_id": "review_loop" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "region-unstick").await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(
        run_state.status,
        event_log::RunStatus::Running,
        "ending the region resumes the halted run"
    );
}

#[tokio::test]
async fn kill_node_requires_node_id() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "kill_node" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn restart_node_requires_node_id() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "restart_node" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn inject_artifact_requires_path_and_content() {
    let state = test_state().await;
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "inject_artifact" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let app2 = build_router(state);
    let resp2 = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/test-run/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "inject_artifact",
                        "path": "some/path.md"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn inject_artifact_rejects_path_traversal() {
    let state = test_state().await;

    for bad_path in [
        "../../etc/passwd",
        "/absolute/path.md",
        "ok/../../../escape",
    ] {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "inject_artifact",
                            "path": bad_path,
                            "content": "malicious"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "path {bad_path:?} should be rejected"
        );
    }
}

#[tokio::test]
async fn inject_artifact_writes_file_and_appends_event() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "inject-test";
    // Create run dir structure
    let artifacts_dir = tmp
        .path()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
        .join(".pdo")
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap();

    let state = test_state_with_dir(tmp.path()).await;

    // Seed a run_started event
    let run_event = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "inject-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "inject_artifact",
                        "path": "manual/iter-1/notes.md",
                        "content": "# Injected\n\nHello from the manager."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify file was written
    let file_path = artifacts_dir.join("manual/iter-1/notes.md");
    assert!(file_path.exists());
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("Injected"));

    // Verify event was appended
    let events = load_events(&state.db, run_id).await.unwrap();
    let cmd_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::CommandIssued)
        .collect();
    assert_eq!(cmd_events.len(), 1);
    let payload = cmd_events[0].payload.as_ref().unwrap();
    assert_eq!(payload["command"], "inject_artifact");
    assert_eq!(payload["path"], "manual/iter-1/notes.md");
}

#[tokio::test]
async fn resume_run_appends_command_issued_event() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "resume-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "resume-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let halt_event = event_log::Event {
        id: None,
        run_id: "resume-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunHalted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "message": "halted" })),
    };
    append_event(&state, &halt_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/resume-test/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "resume_run" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify event was appended and run status changed
    let events = load_events(&state.db, "resume-test").await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Running);
}

#[tokio::test]
async fn restart_node_happy_path_emits_command_issued_and_restarts() {
    // The only arm with no happy-path coverage anywhere in the repo, and the one
    // carrying the most machinery: two projections, a transition-guard probe, a
    // tmux kill and a re-spawn. Pin its event trace.
    //
    // #489: the body is no longer a blind `{"ok":true}`. It reports the real
    // effect — `spawned:[{node_id,iter}]` plus what happened to the
    // sub-worktree — because the arm now READS `spawn_node`'s return.
    // `worker` is non-isolated (`write_test_pipeline`), so it owns no
    // sub-worktree: `reused_sub_worktree:false`, `base_sha:null`. The
    // isolated twin below is the one that can see #489 at all.
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "restart-pipe");
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "restart-happy";
    seed_run_for_node_control(&state, run_id, "restart-pipe").await;

    let (status, ct, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"restart_node","node_id":"worker","iter":1}"#,
    )
    .await;
    assert_eq!((status, ct.as_str()), (StatusCode::OK, JSON_CT));
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["ok"], true, "got {body}");
    assert_eq!(
        parsed["spawned"],
        serde_json::json!([{ "node_id": "worker", "iter": 1 }]),
        "got {body}"
    );
    assert_eq!(parsed["reused_sub_worktree"], false, "got {body}");
    assert!(parsed["base_sha"].is_null(), "got {body}");
    assert_eq!(
        parsed["interrupted_git_ops"],
        serde_json::json!([]),
        "got {body}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    let cmd = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::CommandIssued)
        .expect("restart_node appends a CommandIssued");
    let payload = cmd.payload.as_ref().unwrap();
    assert_eq!(payload["command"], "restart_node");
    assert_eq!(payload["node_id"], "worker");
    assert_eq!(payload["iter"], 1);
    assert_eq!(cmd.node_id.as_deref(), Some("worker"));
    assert_eq!(cmd.iter, Some(1));

    // The re-spawn actually fired: the arm ends with `spawn_node`, whose
    // `NodeStarted` is the only observable proof it ran.
    let started: Vec<_> = events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::NodeStarted)
        .collect();
    assert_eq!(started.len(), 1, "expected one re-spawn: {events:?}");
    assert_eq!(started[0].node_id.as_deref(), Some("worker"));
    assert_eq!(started[0].iter, Some(1));
}

/// CORE-231: a non-isolated node's `NodeStarted` records where the Run's
/// worktree stood (`run_head_sha`): its delivery's `before`, so the commits
/// the node makes itself stay inside its Review pair.
#[tokio::test]
async fn a_non_isolated_spawn_records_the_run_worktree_head() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_test_pipeline(dir, "head-pipe");
    init_test_repo(dir);
    let state = test_state_with_dir(dir).await;
    let run_id = "head-run";
    seed_run_for_node_control(&state, run_id, "head-pipe").await;
    let wt_dir = worktree_dir_for_run(dir, run_id);
    create_worktree(dir, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();
    let head = pdo_workspace::adapters::git::revisions::rev_parse(&wt_dir, "HEAD").unwrap();

    let (status, _, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"restart_node","node_id":"worker","iter":1}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {body}");

    let events = load_events(&state.db, run_id).await.unwrap();
    let started = events
        .iter()
        .rfind(|e| e.kind == event_log::EventKind::NodeStarted)
        .expect("the restart spawned");
    let payload = started.payload.as_ref().unwrap();
    assert_eq!(payload["run_head_sha"], head.as_str(), "{payload}");
    assert!(
        payload["base_sha"].is_null(),
        "no sub-worktree, no cut: {payload}"
    );
}

#[tokio::test]
async fn restart_node_on_an_isolated_node_reuses_its_sub_worktree() {
    // #489, the class that mattered. `write_test_pipeline`'s `worker` is
    // non-isolated, so the repo's only `restart_node` happy path was
    // STRUCTURALLY incapable of seeing this bug: a non-isolated node owns no
    // sub-worktree, and `git worktree add -b` was never replayed.
    //
    // Here the first restart CREATES the sub-worktree and the second one has
    // to REUSE it. Pre-#489 the second call answered `200 {"ok":true}` with
    // `git worktree add -b` having failed 255 and no second `node_started` in
    // the log — the whole issue, in two HTTP calls.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_isolated_node_pipeline(dir, "restart-cm");
    init_test_repo(dir);
    let state = test_state_with_dir(dir).await;
    let run_id = "restart-cm-run";
    seed_run_for_node_control(&state, run_id, "restart-cm").await;

    // The Run's pipeline worktree has to exist: a sub-worktree is cut from
    // `pdo/run-<id>`.
    let wt_dir = worktree_dir_for_run(dir, run_id);
    create_worktree(dir, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();

    let sub_wt_dir =
        pdo_workspace::adapters::git::worktrees::sub_worktree_path(dir, run_id, "worker", 1);
    let body = r#"{"kind":"restart_node","node_id":"worker","iter":1}"#;

    let (status, _, first) = command_triplet(&state, run_id, body).await;
    assert_eq!(status, StatusCode::OK, "first restart: {first}");
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["reused_sub_worktree"], false, "the first cut creates");
    assert!(
        first["base_sha"].as_str().is_some_and(|s| !s.is_empty()),
        "a fresh cut records its base (#503): {first}"
    );
    assert!(sub_wt_dir.exists());

    // Uncommitted work the agent left behind — untracked AND tracked-modified.
    std::fs::write(sub_wt_dir.join("scratch.txt"), "in flight\n").unwrap();
    std::fs::write(sub_wt_dir.join("README.md"), "# edited by the agent\n").unwrap();

    let (status, _, second) = command_triplet(&state, run_id, body).await;
    assert_eq!(status, StatusCode::OK, "second restart: {second}");
    let second: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(
        second["reused_sub_worktree"], true,
        "the second restart must reuse the sub-worktree in place: {second}"
    );
    // #503 / ADR-0036: the base is CARRIED OVER, never re-derived. A missing or
    // re-derived base would disable (or falsely arm) the adoption escape hatch
    // for every restarted node.
    assert_eq!(
        second["base_sha"], first["base_sha"],
        "a reuse cuts nothing, so it reports the ORIGINAL base: {second}"
    );

    // The work survived — this is the guarantee #489-B adds.
    assert_eq!(
        std::fs::read_to_string(sub_wt_dir.join("scratch.txt")).unwrap(),
        "in flight\n"
    );
    assert_eq!(
        std::fs::read_to_string(sub_wt_dir.join("README.md")).unwrap(),
        "# edited by the agent\n"
    );

    // Two real re-spawns, not one plus a lie.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStarted, "worker"),
        2,
        "both restarts must have spawned: {events:?}"
    );
}

#[tokio::test]
async fn resume_run_event_trace_by_state() {
    // Three projected states, three different traces. The `CommandIssued
    // {command:"resume_run"}` Halt-lift is emitted from FOUR sites in this
    // handler — exactly the duplication a refactor is tempted to factor out
    // — and only a negative assertion in the layer-3a suite guards it today.
    // A `Paused` run must NOT get one, and a `Running` run must get nothing
    // at all.
    for (label, seed, want_resumed, want_command) in [
        (
            "resume-from-paused",
            Some(event_log::EventKind::RunPaused),
            true,
            false,
        ),
        (
            "resume-from-halted",
            Some(event_log::EventKind::RunHalted),
            false,
            true,
        ),
        ("resume-from-running", None, false, false),
    ] {
        let state = test_state().await;
        append_event(
            &state,
            &seed_event(
                label,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": "resume-pipe" })),
            ),
        )
        .await
        .unwrap();
        if let Some(kind) = seed {
            append_event(&state, &seed_event(label, kind, None, None, None))
                .await
                .unwrap();
        }

        let (status, ct, _) = command_triplet(&state, label, r#"{"kind":"resume_run"}"#).await;
        assert_eq!((status, ct.as_str()), (StatusCode::OK, JSON_CT), "{label}");

        let events = load_events(&state.db, label).await.unwrap();
        let resumed = events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunResumed);
        let commanded = events.iter().any(|e| {
            e.kind == event_log::EventKind::CommandIssued
                && e.payload
                    .as_ref()
                    .and_then(|p| p.get("command"))
                    .and_then(|v| v.as_str())
                    == Some("resume_run")
        });
        assert_eq!(resumed, want_resumed, "{label}: RunResumed");
        assert_eq!(commanded, want_command, "{label}: CommandIssued resume_run");
    }
}

#[tokio::test]
async fn kill_node_happy_path_event_payloads() {
    // `NodeFailed` rather than `NodeStopped` is the whole point of this arm
    // (a killed node is failed, a stopped one is stopped — two verdicts, two
    // colours in the UI), and `source: "kill_node"` is read downstream. No
    // tmux needed: the kill is best-effort.
    let state = test_state().await;
    let run_id = "kill-happy";
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "kill-pipe" })),
        ),
    )
    .await
    .unwrap();
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
    )
    .await
    .unwrap();

    let (status, ct, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"kill_node","node_id":"worker","iter":1}"#,
    )
    .await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (StatusCode::OK, JSON_CT, r#"{"ok":true}"#)
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    let failed = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::NodeFailed)
        .expect("kill_node marks the node failed, not stopped");
    let payload = failed.payload.as_ref().unwrap();
    assert_eq!(payload["reason"], "killed via kill_node command");
    assert_eq!(payload["source"], "kill_node");
    assert_eq!(failed.node_id.as_deref(), Some("worker"));
    assert_eq!(failed.iter, Some(1));

    let cmd = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::CommandIssued)
        .expect("kill_node appends a CommandIssued");
    let payload = cmd.payload.as_ref().unwrap();
    assert_eq!(payload["command"], "kill_node");
    assert_eq!(payload["node_id"], "worker");
    assert_eq!(payload["iter"], 1);
}

#[tokio::test]
async fn kill_node_without_a_session_creates_no_directory() {
    // The reap writes only when the capture succeeded — `create_dir_all`
    // is INSIDE the `if let Some(content)`. A kill on a node with no session
    // must leave nothing on disk, only in the event log.
    //
    // `test_state_with_dir`, NOT `test_state`: the latter puts `repo_root` at
    // `current_dir()`, i.e. the real source tree. This test is what keeps
    // `kill_node_happy_path_event_payloads` (which does use `test_state`) from
    // ever writing into it.
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "kill-no-session";
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "kill-pipe" })),
        ),
    )
    .await
    .unwrap();
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::NodeStarted,
            Some("worker"),
            Some(1),
            None,
        ),
    )
    .await
    .unwrap();

    let (status, _, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"kill_node","node_id":"worker","iter":1}"#,
    )
    .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, r#"{"ok":true}"#));

    let node_dir = tmp
        .path()
        .join(".pdo/runs")
        .join(run_id)
        .join("nodes/worker");
    assert!(
        !node_dir.exists(),
        "no session means no capture, therefore no directory: {node_dir:?}"
    );
}

#[tokio::test]
async fn parse_rejects_are_ordered_and_field_defaults_are_lenient() {
    // The four judgements a hand-written parser is most likely to reorder or
    // "tidy up". Each is behaviour, not an accident:
    //   * `bump_region` checks `region_id` BEFORE `additional_iter`;
    //   * `inject_artifact` checks `content` BEFORE path traversal;
    //   * `end_region` accepts and ignores a stray `additional_iter`;
    //   * `rename_run` treats a missing `name` as the empty string.
    let state = test_state().await;
    let run_id = "parse-order";
    append_event(
        &state,
        &seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "parse-pipe" })),
        ),
    )
    .await
    .unwrap();

    for (req_body, want) in [
        (
            r#"{"kind":"bump_region"}"#,
            r#"{"error":"region_id required for bump_region"}"#,
        ),
        (
            r#"{"kind":"end_region"}"#,
            r#"{"error":"region_id required for end_region"}"#,
        ),
        (
            r#"{"kind":"inject_artifact","path":"../x"}"#,
            r#"{"error":"content required for inject_artifact"}"#,
        ),
        (
            r#"{"kind":"inject_artifact","content":"x"}"#,
            r#"{"error":"path required for inject_artifact"}"#,
        ),
    ] {
        let (status, ct, body) = command_triplet(&state, run_id, req_body).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (StatusCode::BAD_REQUEST, JSON_CT, want),
            "{req_body}"
        );
    }

    // A stray `additional_iter` on `end_region` is accepted and dropped —
    // the recorded payload carries only the region and who issued it.
    let (status, ct, _) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"end_region","region_id":"r1","additional_iter":5}"#,
    )
    .await;
    assert_eq!((status, ct.as_str()), (StatusCode::OK, JSON_CT));
    let events = load_events(&state.db, run_id).await.unwrap();
    let payload = events
        .iter()
        .rev()
        .find(|e| e.kind == event_log::EventKind::CommandIssued)
        .and_then(|e| e.payload.clone())
        .unwrap();
    assert_eq!(
        payload,
        serde_json::json!({
            "command": "end_region",
            "region_id": "r1",
            "actor": { "kind": "local_operator" },
        })
    );

    // A missing `name` renames to the empty string — 200, not a reject.
    let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"rename_run"}"#).await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (StatusCode::OK, JSON_CT, r#"{"ok":true}"#)
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    let payload = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::RunRenamed)
        .and_then(|e| e.payload.clone())
        .unwrap();
    assert_eq!(
        payload,
        serde_json::json!({ "name": "", "actor": { "kind": "local_operator" } })
    );
}

#[tokio::test]
async fn adr25_case_a_unknown_target_is_404_and_leaves_no_trace() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "adr25-a";
    seed_adr25_run(
        tmp.path(),
        &state,
        run_id,
        "adr25-legacy",
        ADR25_LEGACY_YAML,
        &[],
    )
    .await;

    let (status, ct, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"extend_cycle","node_id":"ghost","additional_iter":2}"#,
    )
    .await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"node_not_found","message":"node 'ghost' not found in the run's pipeline","node_id":"ghost"}"#
        )
    );

    // A rejection leaves NOTHING in the log — validation runs against the
    // snapshot BEFORE any append (ADR-0025).
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued),
        "rejected command must not append: {events:?}"
    );
}

#[tokio::test]
async fn adr25_case_b_wrong_mechanism_is_409_naming_the_region() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "adr25-b";
    seed_adr25_run(
        tmp.path(),
        &state,
        run_id,
        "adr25-loop",
        ADR25_LOOP_YAML,
        &[],
    )
    .await;

    let (status, ct, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"extend_cycle","node_id":"worker","additional_iter":2}"#,
    )
    .await;
    assert_eq!((status, ct.as_str()), (StatusCode::CONFLICT, JSON_CT));
    // The 409 must both NAME the region and say which command to use
    // instead — it is the manager's only routing hint.
    assert!(
        body.contains("'review_loop'") && body.contains("bump_region"),
        "409 must redirect to bump_region by id, got: {body}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued),
        "rejected command must not append: {events:?}"
    );
}

#[tokio::test]
async fn adr25_case_c_noop_carries_a_real_reason() {
    // `worker` iter 1 is live, so the re-evaluation's dedup guard refuses a
    // superfluous spawn and nothing launches. The body must say WHY.
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "adr25-c";
    seed_adr25_run(
        tmp.path(),
        &state,
        run_id,
        "adr25-loop",
        ADR25_LOOP_YAML,
        &[
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("start"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeCompleted,
                Some("start"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                None,
            ),
        ],
    )
    .await;

    let (status, ct, body) = command_triplet(
        &state,
        run_id,
        r#"{"kind":"bump_region","region_id":"review_loop","additional_iter":2}"#,
    )
    .await;
    assert_eq!((status, ct.as_str()), (StatusCode::OK, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["noop"], true,
        "nothing can spawn while the lap is live"
    );
    let reason = body["reason"].as_str().unwrap_or_default();
    assert!(!reason.is_empty(), "noop must carry a reason: {body}");
    // The degenerate noop: an unreadable snapshot ALSO produces a
    // truthful-looking noop while proving nothing about the scheduler.
    // Refuse that pass.
    assert_ne!(
        reason, "pipeline file unreadable",
        "this asserts the guard, not a missing file: {body}"
    );

    // The command is still recorded — it applies when the lap finishes.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == event_log::EventKind::CommandIssued)
            .count(),
        1
    );
}

#[tokio::test]
async fn adr25_case_d_spawned_lists_what_actually_launched() {
    // THE branch nothing in the repo pinned: `{ok, spawned:[{node_id,iter}]}`.
    // The layer-3a tests that touch it accept the disjunction
    // `spawned.is_array() || noop`, so a regression answering `noop`
    // forever would keep the whole suite green.
    //
    // `start` is complete and `worker` has no live iteration, so the
    // re-evaluation has exactly one eligible spawn.
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "adr25-d";
    seed_adr25_run(
        tmp.path(),
        &state,
        run_id,
        "adr25-loop",
        ADR25_LOOP_YAML,
        &[
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("start"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeCompleted,
                Some("start"),
                Some(1),
                None,
            ),
        ],
    )
    .await;

    let before = load_events(&state.db, run_id)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == event_log::EventKind::NodeStarted)
        .count();

    let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"resume_run"}"#).await;
    assert_eq!((status, ct.as_str()), (StatusCode::OK, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["ok"], true);

    let spawned = body["spawned"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a spawned array, got: {body}"));
    assert_eq!(spawned.len(), 1, "one eligible node: {body}");
    assert_eq!(spawned[0]["node_id"], "worker");
    assert_eq!(spawned[0]["iter"], 1);
    assert!(body.get("noop").is_none(), "a spawn is not a noop: {body}");

    // Proof obligation inherited from #108: the body must not claim a spawn
    // the event log does not show.
    let after = load_events(&state.db, run_id)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == event_log::EventKind::NodeStarted)
        .count();
    assert_eq!(
        after - before,
        spawned.len(),
        "one NodeStarted per reported spawn"
    );
}

#[tokio::test]
async fn pause_run_sets_paused_status() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "pause-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/pause-test/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "pause_run" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "pause-test").await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Paused);
}

#[tokio::test]
async fn pause_run_rejected_on_terminal_state() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "pause-completed".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let completed_event = event_log::Event {
        id: None,
        run_id: "pause-completed".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunCompleted,
        node_id: None,
        iter: None,
        payload: None,
    };
    append_event(&state, &completed_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/pause-completed/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "pause_run" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn resume_run_from_paused_emits_run_resumed() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "resume-paused".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
    };
    append_event(&state, &run_event).await.unwrap();

    let pause_event = event_log::Event {
        id: None,
        run_id: "resume-paused".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunPaused,
        node_id: None,
        iter: None,
        payload: None,
    };
    append_event(&state, &pause_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/resume-paused/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "resume_run" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events = load_events(&state.db, "resume-paused").await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Running);

    let has_resumed = events
        .iter()
        .any(|e| e.kind == event_log::EventKind::RunResumed);
    assert!(has_resumed, "should have emitted RunResumed event");
}
