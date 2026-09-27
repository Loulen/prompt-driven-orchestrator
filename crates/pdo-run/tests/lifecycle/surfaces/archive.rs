//! Archiving a Run and its descendants (`cleanup_run`), forgetting it
//! (`DELETE /runs/{id}`), and what an archived Run still answers — moved from
//! the legacy crate with the code they test (#925).

#![allow(unused_imports)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use pretty_assertions::assert_eq;
use tower::ServiceExt;

use super::support::*;
use crate::harness::{append_event, build_router, event_log, load_events, TestState};
use crate::helpers::*;
use pdo_run::adapters::disk_live_facts::{
    augment_run_state_from_disk, compute_run_loc, parse_numstat, run_diff_base,
};
use pdo_run::adapters::env::resolve_max_attachments_mb;
use pdo_run::adapters::http::runs::attachments::{
    sanitize_attachment_filename, sanitize_image_filename,
};
use pdo_run::application::create_run::naming::{
    placeholder_run_name, run_name_hint, validate_run_input,
};
use pdo_run::application::create_run::repos::required_target_repo;
use pdo_run::application::create_run::{
    reap_orphan_run_worktree, reject_unknown_create_run_fields, CreateRunRequest, InputFile,
    CREATE_RUN_FIELDS,
};
use pdo_run::application::events::{append_event_with, run_is_forgotten};
use pdo_run::application::node::prompt as prompt_augmenter;
use pdo_run::application::refs::source_drift::{source_drift, SourceDrift};
use pdo_run::domain::prices as price_table;
use pdo_run::domain::{reap_policy, run_refs};
use pdo_workspace::adapters::git::revisions as structured_diff;
use pdo_workspace::adapters::git::worktrees::{
    self, create_sub_worktree, create_worktree, sub_worktree_branch, sub_worktree_path,
    worktree_dir_for_run,
};

#[tokio::test]
async fn cleanup_run_archives_and_preserves_events() {
    let state = test_state().await;
    let run_id = "cleanup-test-1";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
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
    assert_eq!(body["status"], "archived");

    // Events are preserved — run projects as Archived
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(!events.is_empty());
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Archived);
    // Node state is preserved
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
    // The page mounts its agents published were released with it.
    assert_eq!(
        *state.released_pages.0.lock().unwrap(),
        vec![run_id.to_string()]
    );
}

#[tokio::test]
async fn cleanup_run_already_archived_returns_conflict() {
    let state = test_state().await;
    let run_id = "cleanup-conflict";
    seed_completed_run(&state, run_id).await;

    // First cleanup
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Second cleanup — should conflict
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

/// Seed a run under `parent`: `RunStarted` (with `parent_run_id`), one
/// node started, and — unless `live` — the node completed and the run
/// completed. A live child stays `Running`.
async fn seed_child_run(state: &Arc<TestState>, run_id: &str, parent: &str, live: bool) {
    let mut events = vec![
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "child-pipe",
                "parent_run_id": parent,
                "parent_node_id": "spawner",
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
    ];
    if !live {
        events.push(event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        });
        events.push(event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        });
    }
    for ev in &events {
        append_event(state, ev).await.unwrap();
    }
}

async fn status_of(state: &Arc<TestState>, run_id: &str) -> event_log::RunStatus {
    let events = load_events(&state.db, run_id).await.unwrap();
    event_log::project(&events).unwrap().status
}

/// #815: archiving a parent archives its terminated descendants,
/// recursively (grandchild included), reclaims their run dirs, and does
/// not choke on a descendant that was already archived by hand.
#[tokio::test]
async fn cleanup_run_cascades_to_terminated_descendants() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let state = test_state_with_dir(repo).await;

    let parent = "cascade-parent";
    seed_completed_run(&state, parent).await;
    seed_child_run(&state, "cascade-child-a", parent, false).await;
    seed_child_run(&state, "cascade-child-b", parent, false).await;
    seed_child_run(&state, "cascade-grandchild", "cascade-child-a", false).await;
    seed_child_run(&state, "cascade-archived", parent, false).await;
    // Archived by hand beforehand: the cascade must skip it silently.
    assert_eq!(
        post_cleanup_run(&state, "cascade-archived").await,
        StatusCode::OK
    );
    // An unrelated run must be left alone.
    seed_completed_run(&state, "cascade-stranger").await;

    // Real worktrees for two descendants: the cascade must reclaim them.
    let mut worktrees = Vec::new();
    for id in ["cascade-child-b", "cascade-grandchild"] {
        let wt_dir = repo.join(".pdo/runs").join(id).join("worktree");
        create_worktree(repo, &wt_dir, &format!("pdo/run-{id}"), "HEAD").unwrap();
        assert!(wt_dir.exists());
        worktrees.push(wt_dir);
    }

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{parent}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
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
    assert_eq!(body["status"], "archived");
    // Every non-archived descendant is reported exactly once (the
    // already-archived child is not), and in post-order: the grandchild
    // is torn down before its parent child-a. Sibling order is the
    // store's, not asserted.
    let archived: Vec<&str> = body["archived_children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let mut sorted = archived.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        ["cascade-child-a", "cascade-child-b", "cascade-grandchild"]
    );
    let pos = |id: &str| archived.iter().position(|r| *r == id).unwrap();
    assert!(
        pos("cascade-grandchild") < pos("cascade-child-a"),
        "grandchild must be archived before its parent: {archived:?}"
    );

    for id in [
        parent,
        "cascade-child-a",
        "cascade-child-b",
        "cascade-grandchild",
        "cascade-archived",
    ] {
        assert_eq!(
            status_of(&state, id).await,
            event_log::RunStatus::Archived,
            "{id} must be archived"
        );
    }
    assert_eq!(
        status_of(&state, "cascade-stranger").await,
        event_log::RunStatus::Completed,
        "an unrelated run must not be archived by the cascade"
    );
    for wt_dir in &worktrees {
        assert!(
            !wt_dir.exists(),
            "cascade must remove the descendant worktree {}",
            wt_dir.display()
        );
    }
    for id in ["cascade-child-b", "cascade-grandchild"] {
        assert!(
            !repo.join(".pdo/runs").join(id).exists(),
            "cascade must remove the descendant run dir of {id}"
        );
    }
}

/// #815: a live descendant (even a grandchild) refuses the whole archive
/// with 409 and nothing is archived — not the parent, not the terminated
/// siblings. Stop the child first, then archive.
#[tokio::test]
async fn cleanup_run_refuses_when_a_descendant_is_live() {
    let state = test_state().await;
    let parent = "live-parent";
    seed_completed_run(&state, parent).await;
    seed_child_run(&state, "live-child-done", parent, false).await;
    seed_child_run(&state, "live-child-mid", parent, false).await;
    seed_child_run(&state, "live-grandchild", "live-child-mid", true).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{parent}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
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
    assert_eq!(
        body["live_children"],
        serde_json::json!(["live-grandchild"])
    );
    assert!(body["error"].as_str().unwrap().contains("live child run"));

    assert_eq!(
        status_of(&state, parent).await,
        event_log::RunStatus::Completed
    );
    assert_eq!(
        status_of(&state, "live-child-done").await,
        event_log::RunStatus::Completed
    );
    assert_eq!(
        status_of(&state, "live-child-mid").await,
        event_log::RunStatus::Completed
    );
    assert_eq!(
        status_of(&state, "live-grandchild").await,
        event_log::RunStatus::Running
    );

    // Archiving the live grandchild's own terminated sibling directly still works:
    // the cascade only constrains the subtree being archived.
    assert_eq!(
        post_cleanup_run(&state, "live-child-done").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn forget_run_purges_events_when_archived() {
    let state = test_state().await;
    let run_id = "forget-test-1";
    seed_completed_run(&state, run_id).await;

    // Archive first
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Now forget
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
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
    assert_eq!(body["status"], "forgotten");

    // Event log is empty for this run — projection now returns None
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(events.is_empty(), "event log must be empty after forget");
    assert!(event_log::project(&events).is_none());

    // Forget is durable — a late writer cannot resurrect the run.
    let late = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStopped,
        node_id: Some("n1".into()),
        iter: Some(1),
        payload: None,
    };
    let err = append_event(&state, &late).await.unwrap_err();
    assert!(
        err.to_string().contains("has been forgotten"),
        "unexpected error: {err}"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(events.is_empty(), "late append must not write any event");

    // GET /runs never re-lists the forgotten run.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/runs")
                .body(Body::empty())
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
    assert!(
        !body.to_string().contains(run_id),
        "forgotten run must not reappear in GET /runs: {body}"
    );
}

#[tokio::test]
async fn command_on_forgotten_run_returns_410() {
    let state = test_state().await;
    let run_id = "forget-410";
    seed_completed_run(&state, run_id).await;
    post_cleanup_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // extend_cycle used to append CommandIssued before any existence check
    // — the exact resurrection vector of #328.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "extend_cycle", "node_id": "n1", "additional_iter": 1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);

    // node_done is also rejected before any side effect.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/n1/done"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"iter": 1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(events.is_empty(), "log must stay empty: {events:?}");
}

#[tokio::test]
async fn forget_is_atomic_tombstone() {
    let state = test_state().await;
    let run_id = "forget-tombstone";
    seed_completed_run(&state, run_id).await;
    post_cleanup_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert!(run_is_forgotten(&state.run, run_id).await.unwrap());

    // Re-forgetting a forgotten run: the log is gone, so it 404s (unchanged).
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forget_run_rejects_live_run() {
    let state = test_state().await;
    let run_id = "forget-live";
    seed_completed_run(&state, run_id).await;
    // No cleanup — run is still in `Completed` status, not `Archived`.

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Events are NOT touched
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(!events.is_empty());
}

#[tokio::test]
async fn forget_nonexistent_run_returns_404() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/runs/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cleanup_nonexistent_run_returns_404() {
    let state = test_state().await;
    // The archive is the Run's legacy surface, reached through a `Weak` on
    // the state: the state must outlive the request.
    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/nonexistent/commands")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rename_run_updates_display_name() {
    let state = test_state().await;
    let run_id = "rename-test";
    seed_completed_run(&state, run_id).await;

    // Rename the run
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "rename_run", "name": "My Feature"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify GET /runs/:id returns the name
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
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
    assert_eq!(body["name"], "My Feature");
}

#[tokio::test]
async fn rename_run_appears_in_list() {
    let state = test_state().await;
    let run_id = "rename-list-test";
    seed_completed_run(&state, run_id).await;

    // Rename the run
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"kind": "rename_run", "name": "Listed Name"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify GET /runs returns the name
    let app = build_router(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let entry = body.iter().find(|r| r["run_id"] == run_id).unwrap();
    assert_eq!(entry["name"], "Listed Name");
}

#[tokio::test]
async fn run_without_name_has_null_name_in_list() {
    let state = test_state().await;
    let run_id = "no-name-test";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let entry = body.iter().find(|r| r["run_id"] == run_id).unwrap();
    assert!(entry.get("name").is_none() || entry["name"].is_null());
}

/// The Runs list is where a user first meets a failure, and the entire
/// signal there was a coloured dot. Every `run_failed` had always carried a
/// `reason`; nothing read it, so nothing could title the dot.
#[tokio::test]
async fn the_runs_list_carries_a_failed_run_reason() {
    let state = test_state().await;
    let failed = "failed-with-reason";
    let green = "green-run";
    seed_completed_run(&state, green).await;
    append_event(
        &state,
        &event_log::Event {
            id: None,
            run_id: failed.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
        },
    )
    .await
    .unwrap();
    append_event(
        &state,
        &event_log::Event {
            id: None,
            run_id: failed.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: Some(
                serde_json::json!({ "reason": "merge conflict on ship: 20 conflicting file(s)" }),
            ),
        },
    )
    .await
    .unwrap();

    let resp = build_router(state.clone())
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let entry = body.iter().find(|r| r["run_id"] == failed).unwrap();
    assert_eq!(
        entry["failure_reason"],
        "merge conflict on ship: 20 conflicting file(s)"
    );
    // A green run carries no reason and no empty key to render.
    let entry = body.iter().find(|r| r["run_id"] == green).unwrap();
    assert!(entry.get("failure_reason").is_none());
}

#[tokio::test]
async fn create_run_with_name() {
    let state = test_state().await;
    let run_id = "named-create";

    // Seed a run with a name in the RunStarted payload
    let event = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "hello",
            "name": "Named Run"
        })),
    };
    append_event(&state, &event).await.unwrap();

    // Verify GET /runs/:id returns the name
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
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
    assert_eq!(body["name"], "Named Run");
}

#[tokio::test]
async fn archived_run_still_appears_in_list() {
    let state = test_state().await;
    let run_id = "list-archive-test";
    seed_completed_run(&state, run_id).await;

    // Archive it
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // List runs — should still include the archived run
    let app = build_router(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"], "archived");

    // GET /runs/:id/events still returns full history
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(events.len() >= 5); // run_started + node_started + node_completed + run_completed + run_archived
}

#[tokio::test]
async fn archived_run_with_null_target_repo_stays_readable() {
    // #470/AC6: the write boundary hardens, the READ side does not. An archived
    // run whose `run_started` legitimately carries `target_repo: null` (≈46 of
    // 101 dev runs) must stay fully readable forever — detail and list — with
    // its repo resolved to the daemon root. This is the ADR-0033 asymmetry.
    let state = test_state().await;
    let run_id = "legacy-null-archived";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Detail: 200, not a 4xx/5xx from a "required" field that isn't there.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "an archived null-target run must stay readable"
    );

    // List: present, and grouped under the daemon root — no "Unassigned".
    let app = build_router(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let row = runs
        .iter()
        .find(|r| r["run_id"] == run_id)
        .expect("the archived run must still be listed");
    assert_eq!(row["status"], "archived");
    assert_eq!(
        row["effective_repo"].as_str(),
        state.repo_root.to_str(),
        "a null-target archived run resolves to the daemon root, not Unassigned"
    );
}

#[tokio::test]
async fn cleanup_run_removes_surviving_sub_worktrees_and_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "cleanup-sub-wt";
    let state = test_state_with_dir(repo).await;

    seed_run_with_node(&state, run_id, "impl-1", true).await;
    for ev in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("impl-1".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        },
    ] {
        append_event(&state, &ev).await.unwrap();
    }

    // Create real worktrees on disk (simulating what the daemon would do)
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

    std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();
    let result =
        commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
    assert!(matches!(result, worktrees::MergeResult::Success));

    // Sub-worktree must exist before cleanup (refs #32)
    assert!(sub_wt_dir.exists());

    // Run cleanup_run via the HTTP endpoint
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Sub-worktree directory removed
    assert!(
        !sub_wt_dir.exists(),
        "cleanup_run must remove sub-worktree directory"
    );

    // Pipeline worktree directory removed
    assert!(
        !wt_dir.exists(),
        "cleanup_run must remove pipeline worktree directory"
    );

    // Sub-branch removed
    let branch_check = std::process::Command::new("git")
        .args(["branch", "--list", &sub_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&branch_check.stdout);
    assert!(
        !branches.contains(&sub_branch),
        "cleanup_run must remove sub-branch; got: {branches}"
    );

    // Pipeline branch removed
    let branch_check = std::process::Command::new("git")
        .args(["branch", "--list", &pipeline_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&branch_check.stdout);
    assert!(
        !branches.contains(&pipeline_branch),
        "cleanup_run must remove pipeline branch; got: {branches}"
    );

    // Events are preserved
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(!events.is_empty());
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Archived);
}


/// R1: archiving a run copies `worktree/.pdo/artifacts` + `pipeline.yaml` to
/// the durable global dir `$HOME/.pdo/runs/<id>/` BEFORE the worktree is
/// destroyed, and the run still projects `Archived`.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn cleanup_run_preserves_artifacts_to_durable_dir() {
    let home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "preserve-durable";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    // Sanity: the live artifacts exist before archive; the durable copy does not.
    let run_dir = tmp.path().join(".pdo").join("runs").join(run_id);
    assert!(run_dir
        .join("worktree/.pdo/artifacts/planner/iter-1/plan/output.md")
        .exists());
    let durable = home.path().join(".pdo").join("runs").join(run_id);
    assert!(
        !durable.exists(),
        "durable dir must not exist before archive"
    );

    assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

    // The repo-local run dir (worktree + artifacts) is gone …
    assert!(
        !run_dir.exists(),
        "cleanup must remove the repo-local run dir"
    );
    // … but the durable copy holds the artifacts + pipeline.yaml.
    let preserved_plan = durable.join("artifacts/planner/iter-1/plan/output.md");
    assert!(
        preserved_plan.exists(),
        "planner output must be preserved to the durable dir"
    );
    assert_eq!(
        std::fs::read_to_string(&preserved_plan).unwrap(),
        "# Plan\nDo stuff."
    );
    assert!(
        durable
            .join("artifacts/implementer/iter-1/summary/output.md")
            .exists(),
        "implementer output must be preserved"
    );
    assert!(
        durable.join("pipeline.yaml").exists(),
        "pipeline.yaml must be preserved to the durable dir"
    );

    // Events survive; run projects Archived.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::Archived
    );
}

/// #408 (F4): `cleanup_run` merges a sandboxed Run's staged transcripts into
/// `~/.claude/projects/` (recursively — subagent jsonl included, non-jsonl
/// siblings excluded) BEFORE the staging is purged, via the real route. This
/// is the tmux-independent proof that `cleanup_run` → `run_sandbox::cleanup` →
/// `merge_back` is wired; the synchronous cleanup merge means no waiting.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn cleanup_run_merges_sandbox_transcripts_to_host() {
    let home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "cleanup-merge";
    // `docker rm -f` → `true` (no-op), so no real docker is ever spawned.
    let state = test_state_with_dir_and_docker(tmp.path(), Some("true".to_string())).await;

    // Seed a sandboxed (`full`) run: RunStarted projects `sandbox=full`.
    append_event_with(
        &state.run,
        &event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "sbx-obs",
                "input": "x",
                "sandbox": "full",
            })),
        },
    )
    .await
    .unwrap();

    // Plant a staged transcript tree: a top-level jsonl, a nested subagent
    // jsonl (locks the recursion), and a non-jsonl sibling that must NOT land.
    let staged_projects = home
        .path()
        .join(".pdo/sandbox")
        .join(run_id)
        .join("claude-home/projects");
    let enc = "-some-enc-worktree";
    let proj = staged_projects.join(enc);
    std::fs::create_dir_all(proj.join("uuid-1/subagents")).unwrap();
    std::fs::write(proj.join("main.jsonl"), "{\"line\":1}\n").unwrap();
    std::fs::write(proj.join("uuid-1/subagents/side.jsonl"), "{\"line\":2}\n").unwrap();
    std::fs::write(proj.join("notes.txt"), "not a transcript").unwrap();

    assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

    // Merged to the host projects dir, recursively, jsonl-only.
    let host = home.path().join(".claude/projects").join(enc);
    assert_eq!(
        std::fs::read_to_string(host.join("main.jsonl")).unwrap(),
        "{\"line\":1}\n",
        "top-level transcript must be merged verbatim"
    );
    assert_eq!(
        std::fs::read_to_string(host.join("uuid-1/subagents/side.jsonl")).unwrap(),
        "{\"line\":2}\n",
        "nested subagent transcript must be merged (recursion)"
    );
    assert!(
        !host.join("notes.txt").exists(),
        "non-jsonl siblings must NOT be merged"
    );
    // Staging purged after the merge; run Archived.
    assert!(
        !home.path().join(".pdo/sandbox").join(run_id).exists(),
        "cleanup must purge the staging after merging"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::Archived
    );
}

/// R2: after archive (worktree gone), `GET /nodes/<n>/io` serves the correct
/// payload from the durable store — the shape matches the live-run contract.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn node_io_serves_archived_run_from_durable_dir() {
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "io-archived";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);
    // The live worktree is gone — the only source left is the durable dir.
    assert!(!tmp
        .path()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
        .exists());

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/implementer/io?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let io: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Input (planner's plan) resolves from the preserved tree.
    let inputs = io["inputs"].as_array().unwrap();
    assert_eq!(inputs[0]["port"], "plan");
    assert_eq!(inputs[0]["files"][0]["exists"], true);
    // Output (summary) with its frontmatter parsed from the preserved file.
    let outputs = io["outputs"].as_array().unwrap();
    assert_eq!(outputs[0]["port"], "summary");
    assert_eq!(outputs[0]["files"][0]["exists"], true);
    assert_eq!(outputs[0]["files"][0]["frontmatter"]["verdict"], "PASS");
    assert_eq!(outputs[0]["files"][0]["frontmatter"]["score"], 9);
}

/// R3: after archive, `GET /artifact` serves the preserved bytes with the
/// right MIME, and the path-traversal guard still holds against the durable base.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn artifact_serves_archived_run_content() {
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-archived";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/artifact?path=planner/iter-1/plan/output.md"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "text/markdown");
    // #796: never cached — a re-spawned node rewrites the same URL.
    assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("# Plan"));

    // Traversal is still rejected against the durable base.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/artifact?path=../../../../../../etc/passwd"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
        "traversal against durable base must be rejected, got {}",
        resp.status()
    );
}

/// R4: forgetting an archived run reclaims the durable store — no leak.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn forget_run_removes_durable_dir() {
    let home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "forget-durable";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);
    let durable = home.path().join(".pdo").join("runs").join(run_id);
    assert!(durable.exists(), "durable dir must exist after archive");

    // Forget the run.
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert!(
        !durable.exists(),
        "forget must reclaim the durable dir — no leak"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(events.is_empty(), "events must be purged after forget");
}

// -----------------------------------------------------------------------
// Declared wait (#588 / ADR-0069)
// -----------------------------------------------------------------------

/// ADR-0024's tombstone keeps its `410` on this route: "never a 2xx" is not
/// "always a 409", and flattening these would be the wrong reading of #490.
#[tokio::test]
async fn a_forgotten_run_still_answers_410_on_the_done_route() {
    let state = test_state().await;
    let run_id = "done-forgotten";
    seed_completed_run(&state, run_id).await;
    post_cleanup_run(&state, run_id).await;
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (status, ct, raw) = done_triplet(&state, run_id, "worker", "{}").await;
    assert_eq!((status, ct.as_str()), (StatusCode::GONE, JSON_CT));
    let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(body["error"], "run_forgotten");
    assert_eq!(body["recoverable"], false);
}

#[test]
fn reap_orphan_run_worktree_removes_empty_run_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let worktree = worktree_dir_for_run(repo, "failed-run");
    reap_orphan_run_worktree(repo, &worktree, "pdo/run-failed-run");

    assert!(
        !worktree.parent().unwrap().exists(),
        "an aborted Run must not leave its empty directory behind"
    );
}
