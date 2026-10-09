//! The fixtures the review's lifecycle tests share: a git repository, the
//! Runs they seed into the log, the comments they send straight into it, and
//! the requests they make.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_review::domain::comments::{ReviewComment, ReviewCommentStatus, ReviewSide};
use pdo_workspace::adapters::git::worktrees::create_worktree;
use tower::ServiceExt;

use crate::harness::{append_event, event_log, TestState};

pub fn init_test_repo(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    run(&["init"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(dir.join("README.md"), "# test\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "initial"]);
}

/// Seed a Run whose single `agent` node has already started.
pub async fn seed_run_with_node(
    state: &Arc<TestState>,
    run_id: &str,
    node_id: &str,
    isolated: bool,
) {
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "test",
            "node_defs": [
                {
                    "id": node_id,
                    "node_type": "agent",
                    "isolated_worktree": isolated,
                    "inputs": [],
                    "outputs": []
                }
            ],
            "edges": []
        })),
    };
    append_event(state, &run_started).await.unwrap();
    let node_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some(node_id.into()),
        iter: Some(1),
        payload: Some(serde_json::json!({ "node_type": "agent", "isolated_worktree": isolated })),
    };
    append_event(state, &node_started).await.unwrap();
}

/// Seed a Run that started, ran one node and completed.
pub async fn seed_completed_run(state: &Arc<TestState>, run_id: &str) {
    for (kind, node_id, iter, payload) in [
        (
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
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
        (event_log::EventKind::RunCompleted, None, None, None),
    ] {
        let event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: node_id.map(str::to_string),
            iter,
            payload,
        };
        append_event(state, &event).await.unwrap();
    }
}

/// #748 fixture: a Run whose `target_repo` is a DIFFERENT git repo than the
/// daemon's `repo_root`. The daemon root is a bare-ish repo with no run
/// branch at all, so anything computed in the cwd answers 404 / empty —
/// the exact pre-#748 symptom. Returns `(target_repo, worktree_dir)`.
pub async fn seed_run_in_other_repo(
    state: &Arc<TestState>,
    target: &std::path::Path,
    run_id: &str,
) -> PathBuf {
    init_test_repo(target);
    let fork_sha = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(target)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "test",
            "target_repo": target.to_string_lossy(),
            "fork_sha": fork_sha,
            "node_defs": [
                { "id": "impl-1", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] }
            ],
            "edges": []
        })),
    };
    append_event(state, &run_started).await.unwrap();
    let node_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some("impl-1".into()),
        iter: Some(1),
        payload: Some(serde_json::json!({ "node_type": "agent", "isolated_worktree": true })),
    };
    append_event(state, &node_started).await.unwrap();

    let wt_dir = target.join(".pdo/runs").join(run_id).join("worktree");
    create_worktree(target, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();
    wt_dir
}

pub fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub async fn body_string(resp: Response) -> String {
    String::from_utf8(
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

/// Append one `review_comment_sent` straight into the log — the send endpoint
/// needs git + tmux, which these handler tests do not.
pub async fn seed_sent_comment(state: &Arc<TestState>, run_id: &str, id: &str) {
    let c = ReviewComment {
        id: id.into(),
        path: "lib.rs".into(),
        side: ReviewSide::New,
        line: 4,
        from_ref: "fork".into(),
        to_ref: "tip".into(),
        from_sha: None,
        to_sha: None,
        text: "Name this `fn delta`.".into(),
        excerpt: String::new(),
        author: "user".into(),
        actor: None,
        sent_at: pdo_kernel::clock::now_iso(),
        batch_id: Some("b1".into()),
        status: ReviewCommentStatus::Sent,
        replies: vec![],
        proposal_pending: false,
        resolved_by: None,
        resolved_at: None,
        resolved_by_actor: None,
        reopened_by: None,
        reopened_at: None,
        reopened_by_actor: None,
        proposal_declined: false,
    };
    append_event(
        state,
        &event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::ReviewCommentSent,
            node_id: None,
            iter: None,
            payload: Some(serde_json::to_value(&c).unwrap()),
        },
    )
    .await
    .unwrap();
}

pub async fn review_post(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
    session: Option<(&str, &str)>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some((run, node)) = session {
        req = req
            .header(SESSION_RUN_HEADER, run)
            .header(SESSION_NODE_HEADER, node);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

pub async fn review_get(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
