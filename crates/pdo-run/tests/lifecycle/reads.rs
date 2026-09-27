//! What a node reads: its prompt, its inputs and outputs, its diff; and the `/ws` stream.

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
use pdo_run::application::merge_resolver::MERGE_RESOLVER_NODE_ID;
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
async fn ws_connects_and_receives_ready() {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;

    let state = test_state().await;
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "ready");
}

#[tokio::test]
async fn websocket_receives_events() {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;

    let state = test_state().await;
    let app = build_router(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = connect_async(&url).await.unwrap();

    // Consume the "ready" message
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "ready");

    // Append an event and check it arrives on the WebSocket
    let event = event_log::Event {
        id: None,
        run_id: "ws-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "ws-pipe" })),
    };
    append_event(&state, &event).await.unwrap();

    // Read the next message — should be the event (skip heartbeats)
    let deadline = time::Instant::now() + Duration::from_secs(10);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("timeout waiting for event")
            .unwrap()
            .unwrap();
        let text = msg.into_text().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        if parsed["type"] == "event" {
            assert_eq!(parsed["event"]["kind"], "run_started");
            break;
        }
    }
}

#[tokio::test]
async fn node_def_isolation_projects_from_the_run_snapshot() {
    let state = test_state().await;
    let run_id = "test-cm-node-done";

    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "cm-test",
            "input": "test",
            "node_defs": [
                { "id": "impl-1", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] }
            ],
            "edges": []
        })),
    };
    append_event(&state, &run_started).await.unwrap();

    let node_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some("impl-1".into()),
        iter: Some(1),
        payload: Some(serde_json::json!({ "node_type": "agent", "isolated_worktree": true })),
    };
    append_event(&state, &node_started).await.unwrap();

    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(find_node_type(&run_state, "impl-1"), Some("agent"));
    // #653/ADR-0060: the snapshot carries where the node works, and the
    // NodeRun's own frozen answer agrees with it on a first spawn.
    assert!(snapshot_isolation(&run_state, "impl-1"));
    assert!(node_run_isolation(&events, &run_state, "impl-1", 1));
}

#[tokio::test]
async fn node_prompt_returns_404_for_nonexistent_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/no-such-run/nodes/worker/prompt?iter=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_prompt_returns_404_for_nonexistent_node() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "prompt-test-no-node";
    seed_run_with_node(&state, run_id, "worker", false).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/nonexistent/prompt?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_prompt_returns_404_when_file_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "prompt-test-no-file";
    seed_run_with_node(&state, run_id, "worker", false).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/worker/prompt?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_prompt_returns_markdown_when_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "prompt-test-ok";
    let wt_dir = tmp.path().join(".pdo/runs").join(run_id).join("worktree");
    let prompt_dir = wt_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir).unwrap();
    std::fs::write(
        prompt_dir.join("worker-iter-1.md"),
        "## Inputs\n\n- task: /path/to/task.md\n\n## Outputs\n\n- result\n",
    )
    .unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    seed_run_with_node(&state, run_id, "worker", false).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/worker/prompt?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/markdown"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("## Inputs"));
    assert!(text.contains("## Outputs"));
}

#[tokio::test]
async fn node_prompt_defaults_iter_to_1() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "prompt-test-default-iter";
    let wt_dir = tmp.path().join(".pdo/runs").join(run_id).join("worktree");
    let prompt_dir = wt_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir).unwrap();
    std::fs::write(prompt_dir.join("worker-iter-1.md"), "prompt content").unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    seed_run_with_node(&state, run_id, "worker", false).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/worker/prompt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn prompt_endpoint_returns_200_for_completed_cm_node_iter() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "prompt-cm-survive";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    // Create real worktrees
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

    // Write prompt file in the sub-worktree (as the daemon does at spawn)
    let prompt_dir = sub_wt_dir.join(".pdo").join("prompts");
    std::fs::create_dir_all(&prompt_dir).unwrap();
    std::fs::write(
        prompt_dir.join("impl-1-iter-1.md"),
        "## Inputs\n\n## Outputs\n\nYou are an implementer.\n",
    )
    .unwrap();

    // Merge sub-worktree (simulating node completion)
    std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();
    let result =
        commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
    assert!(matches!(result, worktrees::MergeResult::Success));

    // Prompt endpoint must return 200 for the completed iter
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/impl-1/prompt?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "prompt endpoint must return 200 for completed isolated node iter"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.is_empty(), "prompt body must be non-empty");
    assert!(text.contains("## Inputs"));
}

#[tokio::test]
async fn node_io_returns_404_for_missing_run() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/nope/nodes/worker/io?iter=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_io_returns_404_for_missing_node() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "io-missing-node";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/ghost/io?iter=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_io_returns_correct_payload_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "io-shape-test";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
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

    // Inputs array
    let inputs = io["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["port"], "plan");
    assert_eq!(inputs[0]["repeated"], false);
    assert!(!inputs[0]["files"].as_array().unwrap().is_empty());
    assert_eq!(inputs[0]["files"][0]["exists"], true);

    // Outputs array
    let outputs = io["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0]["port"], "summary");
    assert_eq!(outputs[0]["files"][0]["exists"], true);
    // Frontmatter parsed
    let fm = &outputs[0]["files"][0]["frontmatter"];
    assert_eq!(fm["verdict"], "PASS");
    assert_eq!(fm["score"], 9);
}

#[tokio::test]
async fn node_io_defaults_iter_to_1() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "io-default-iter";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/implementer/io"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn node_diff_returns_404_for_nonexistent_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/no-such-run/nodes/impl-1/diff")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_diff_returns_per_node_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-node";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    // Create a sub-worktree for impl-1
    let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

    // Make a change in the sub-worktree
    std::fs::write(sub_wt_dir.join("node_file.rs"), "fn node_work() {}\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&sub_wt_dir)
            .output()
            .unwrap()
    };
    run(&["add", "node_file.rs"]);
    run(&["commit", "-m", "node impl"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/impl-1/diff"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("node_file.rs"),
        "diff should mention node_file.rs"
    );
    assert!(
        body.contains("fn node_work()"),
        "diff should contain the node's changes"
    );
}

#[tokio::test]
async fn node_diff_returns_404_for_nonexistent_node() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-no-node";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/nonexistent/diff"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn node_diff_computes_in_effective_repo_not_daemon_cwd() {
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "node-diff-other-repo";
    let state = test_state_with_dir(daemon_dir.path()).await;
    seed_run_in_other_repo(&state, target, run_id).await;

    // The node's live sub-worktree branch, in the TARGET repo.
    let pipeline_branch = format!("pdo/run-{run_id}");
    let sub_wt_dir = sub_worktree_path(target, run_id, "impl-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(target, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();
    std::fs::write(sub_wt_dir.join("node_file.rs"), "fn node_work() {}\n").unwrap();
    git_in(&sub_wt_dir, &["add", "node_file.rs"]);
    git_in(&sub_wt_dir, &["commit", "-q", "-m", "node impl"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/nodes/impl-1/diff"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("node_file.rs") && body.contains("fn node_work()"),
        "{body}"
    );
}
