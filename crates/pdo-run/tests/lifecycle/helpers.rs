//! The seeds, fixtures and probes the lifecycle tests share (moved with them
//! from the legacy crate, #924).

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

pub const ADR25_LOOP_YAML: &str = r#"name: adr25-loop
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
loops:
  - id: review_loop
    kind: bounded
    members: [worker]
    max_iter: 3
"#;

pub const ADR25_LEGACY_YAML: &str = r#"name: adr25-legacy
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: planner
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
"#;

/// Seed a run whose pipeline SNAPSHOT is the given YAML, plus its events.
pub async fn seed_adr25_run(
    dir: &std::path::Path,
    state: &Arc<TestState>,
    run_id: &str,
    pipeline_name: &str,
    yaml: &str,
    events: &[event_log::Event],
) {
    let run_dir = dir.join(".pdo").join("runs").join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("pipeline.yaml"), yaml).unwrap();
    append_event(
        state,
        &seed_event(
            run_id,
            event_log::EventKind::RunStarted,
            None,
            None,
            Some(serde_json::json!({ "pipeline_name": pipeline_name })),
        ),
    )
    .await
    .unwrap();
    for ev in events {
        append_event(state, ev).await.unwrap();
    }
}

pub const JSON_CT: &str = "application/json";

pub const START_END_YAML: &str = "  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

/// Append an event carrying an explicit (here: backdated) timestamp, so a
/// test can make a run read as "idle for N seconds" without sleeping.
pub async fn append_event_at(
    state: &Arc<TestState>,
    run_id: &str,
    ts: &str,
    kind: event_log::EventKind,
    node_id: Option<&str>,
    iter: Option<i64>,
    payload: Option<serde_json::Value>,
) {
    let ev = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: ts.into(),
        kind,
        node_id: node_id.map(Into::into),
        iter,
        payload,
    };
    append_event(state, &ev).await.unwrap();
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

/// POST a command and return `(status, content-type, body)` — the triplet
/// that IS the wire contract. Order matters: `into_body()` consumes the
/// response, so status and headers must be read first.
pub async fn command_triplet(
    state: &Arc<TestState>,
    run_id: &str,
    body: &str,
) -> (StatusCode, String, String) {
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .into_owned();
    (status, content_type, body)
}

/// The merge-back on its **conservative** arm: no spawn base, so a conflict still reads `MergeResult::Conflict`.
pub fn commit_and_merge_sub_worktree(
    sub_worktree_dir: &Path,
    pipeline_worktree_dir: &Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
) -> anyhow::Result<worktrees::MergeResult> {
    worktrees::commit_and_merge_sub_worktree_inner(
        sub_worktree_dir,
        pipeline_worktree_dir,
        sub_branch,
        node_id,
        iter,
        None,
        &pdo_workspace::adapters::git::worktrees::CommitIdentity::default(),
    )
}

pub fn count_events(
    events: &[event_log::Event],
    kind: event_log::EventKind,
    node_id: &str,
) -> usize {
    events
        .iter()
        .filter(|e| e.kind == kind && e.node_id.as_deref() == Some(node_id))
        .count()
}

pub fn count_run_completed(events: &[event_log::Event]) -> usize {
    events
        .iter()
        .filter(|e| e.kind == event_log::EventKind::RunCompleted)
        .count()
}

/// `command_triplet`'s sibling for `POST …/nodes/:id/done`. Status **and**
/// content-type, so the two surfaces are pinned by the same shape of assertion
/// (net of #236).
pub async fn done_triplet(
    state: &Arc<TestState>,
    run_id: &str,
    node_id: &str,
    body: &str,
) -> (StatusCode, String, String) {
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/{node_id}/done"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .into_owned();
    (status, content_type, body)
}

/// The `reason` prose carried by the first event of `kind` for `run_id`.
pub async fn event_reason(
    state: &Arc<TestState>,
    run_id: &str,
    kind: event_log::EventKind,
) -> String {
    let events = load_events(&state.db, run_id).await.unwrap();
    events
        .iter()
        .find(|e| e.kind == kind)
        .and_then(|e| e.payload.as_ref())
        .and_then(|p| p.get("reason"))
        .and_then(|r| r.as_str())
        .unwrap_or_default()
        .to_string()
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

/// A throwaway git repo with one tracked file, for the two refusals that need a
/// real worktree. Same shape as `worktree_ops`' own `init_test_repo`, which is
/// private to that module's test tree.
pub fn init_git_repo_for_refusal_tests(repo: &std::path::Path) {
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "refusal@test.local"]);
    git(&["config", "user.name", "Refusal Test"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n").unwrap();
    std::fs::write(repo.join("tracked.txt"), "immutable\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
}

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

/// `mark_node_done` through the command surface, as a triplet.
pub async fn mark_done_triplet(
    state: &Arc<TestState>,
    run_id: &str,
    node_id: &str,
) -> (StatusCode, String, String) {
    command_triplet(
        state,
        run_id,
        &serde_json::json!({ "kind": "mark_node_done", "node_id": node_id, "iter": 1 }).to_string(),
    )
    .await
}

/// #508 hermetic seam: make `tmux_session_manager::spawn` fail at its very
/// first statement (`create_dir_all(working_dir/.pdo/prompts)`) by planting a
/// *file* named `.pdo/prompts` where a directory is expected. `create_dir_all`
/// returns `AlreadyExists` (os error 17) BEFORE tmux is ever invoked, for
/// every node type — no real tmux, no debug poison. `commit` decides whether
/// the file is only on disk (a non-isolated node runs straight in `worktree_dir`)
/// or committed onto HEAD (an isolated node's sub-worktree is a fresh
/// checkout of the pipeline branch that must carry the collision file).
pub fn plant_pdo_prompts_file(dir: &std::path::Path, commit: bool) {
    let pdo = dir.join(".pdo");
    std::fs::create_dir_all(&pdo).unwrap();
    std::fs::write(pdo.join("prompts"), "collision\n").unwrap();
    if commit {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["add", "-f", ".pdo/prompts"]);
        run(&[
            "commit",
            "-m",
            "seed .pdo/prompts collision file (#508 test)",
        ]);
    }
}

pub async fn response_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// Seed a `blocker` run holding enough Running nodes to saturate the
/// global session cap, so any legitimate spawn in the run under test lands
/// as an observable `node_waiting` event instead of a real tmux session.
pub async fn saturate_session_cap(state: &Arc<TestState>) {
    let run_id = "cap-blocker";
    let run_started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": "blocker" })),
    };
    append_event(state, &run_started).await.unwrap();
    for i in 0..admission::DEFAULT_SESSION_CAP {
        let started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(format!("hog-{i}")),
            iter: Some(1),
            payload: None,
        };
        append_event(state, &started).await.unwrap();
    }
}

pub async fn seed_awaiting_run(state: &Arc<TestState>, run_id: &str, pipeline_name: &str) {
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
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
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(state, &event).await.unwrap();
    }
}

pub async fn seed_completed_run(state: &Arc<TestState>, run_id: &str) {
    let events = vec![
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
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
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        },
    ];
    for ev in &events {
        append_event(state, ev).await.unwrap();
    }
}

pub fn seed_event(
    run_id: &str,
    kind: event_log::EventKind,
    node_id: Option<&str>,
    iter: Option<i64>,
    payload: Option<serde_json::Value>,
) -> event_log::Event {
    event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: node_id.map(String::from),
        iter,
        payload,
    }
}

pub async fn seed_failed_run(state: &Arc<TestState>, run_id: &str, pipeline_name: &str) {
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
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
            kind: event_log::EventKind::NodeFailed,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "reason": "tool call exited 1" })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "reason": "tool call exited 1" })),
        },
    ] {
        append_event(state, &event).await.unwrap();
    }
}

pub async fn seed_interactive_node(
    state: &Arc<TestState>,
    run_id: &str,
    status: event_log::EventKind,
) {
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
            kind: status,
            node_id: Some("griller".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(state, &event).await.unwrap();
    }
}

pub async fn seed_io_run_events(state: &Arc<TestState>, run_id: &str) {
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "io-test-pipe",
                "input": "test",
                "node_defs": [
                    { "id": "planner", "node_type": "agent", "isolated_worktree": false, "inputs": ["task"], "outputs": ["plan"] },
                    { "id": "implementer", "node_type": "agent", "isolated_worktree": true, "inputs": ["plan"], "outputs": ["summary"] }
                ],
                "edges": [
                    { "source_node": "planner", "source_port": "plan", "target_node": "implementer", "target_port": "plan" }
                ]
            })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("planner".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("planner".into()),
            iter: Some(1),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("implementer".into()),
            iter: Some(1),
            payload: None,
        },
    ] {
        append_event(state, &event).await.unwrap();
    }
}

pub fn seed_io_test(dir: &std::path::Path, run_id: &str) {
    let run_dir = dir.join(".pdo/runs").join(run_id);
    let pipeline_path = run_dir.join("pipeline.yaml");
    std::fs::create_dir_all(run_dir.join("worktree/.pdo/artifacts/planner/iter-1/plan")).unwrap();
    std::fs::create_dir_all(run_dir.join("worktree/.pdo/artifacts/implementer/iter-1/summary"))
        .unwrap();
    std::fs::create_dir_all(pipeline_path.parent().unwrap()).unwrap();
    std::fs::write(
            &pipeline_path,
            format!(
                "name: io-test-pipe\nnodes:\n{START_END_YAML}  - id: planner\n    name: planner\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: plan\n  - id: implementer\n    name: implementer\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: plan\n    outputs:\n      - name: summary\nedges:\n  - source: {{ node: planner, port: plan }}\n    target: {{ node: implementer, port: plan }}\n"
            ),
        )
        .unwrap();
    std::fs::write(
        run_dir.join("worktree/.pdo/artifacts/planner/iter-1/plan/output.md"),
        "# Plan\nDo stuff.",
    )
    .unwrap();
    std::fs::write(
        run_dir.join("worktree/.pdo/artifacts/implementer/iter-1/summary/output.md"),
        "---\nverdict: PASS\nscore: 9\n---\n\n## Summary\nAll good.",
    )
    .unwrap();
}

pub async fn seed_node_completed(state: &Arc<TestState>, run_id: &str, node_id: &str, iter: i64) {
    let ev = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.into()),
        iter: Some(iter),
        payload: None,
    };
    append_event(state, &ev).await.unwrap();
}

pub async fn seed_node_started(state: &Arc<TestState>, run_id: &str, node_id: &str, iter: i64) {
    let ev = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some(node_id.into()),
        iter: Some(iter),
        payload: None,
    };
    append_event(state, &ev).await.unwrap();
}

pub async fn seed_run_for_node_control(state: &Arc<TestState>, run_id: &str, pipeline_name: &str) {
    let ev = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
    };
    append_event(state, &ev).await.unwrap();
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

/// Append a run-level terminal event directly (not lifecycle-guarded), so a
/// test can drive a seeded run to a terminal status without spawning nodes.
pub async fn seed_run_terminal(state: &Arc<TestState>, run_id: &str, kind: event_log::EventKind) {
    let ev = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: None,
        iter: None,
        payload: None,
    };
    append_event(state, &ev).await.unwrap();
}

/// Seed a Run whose single `agent` node has already started, with the
/// isolation (#653) that decides where its NodeRun works.
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

/// Seed a sandboxed Run whose prep has STARTED but not finished — the projection
/// the watcher used to spawn into (`sandbox_prep = pending`).
pub async fn seed_sandboxed_run_mid_prep(
    state: &Arc<TestState>,
    run_id: &str,
    pipeline_name: &str,
) {
    let started = event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": pipeline_name,
            "sandbox": "full",
            "sandbox_entries": [".claude/plugins"],
        })),
    };
    append_event(state, &started).await.unwrap();
    emit_run_event(
        &state.run,
        run_id,
        event_log::EventKind::SandboxPrepStarted,
        None,
    )
    .await;
}

/// Minimal `SpawnContext` scaffolding shared by the direct-drive tests: a
/// one-node pipeline plus the four dir borrows. Returns owned values the
/// caller keeps alive; the `SpawnContext` is built at the call site from
/// borrows of these.
/// An `agent` node and its one-node pipeline. `isolated` (#653) is what
/// decides whether the spawn cuts a sub-worktree — the axis the retired
/// non-isolated / isolated types used to stand in for here.
pub fn spawn_test_fixture(
    repo_root: &std::path::Path,
    node_id: &str,
    isolated: bool,
) -> (pipeline::PipelineDef, pipeline::NodeDef, std::path::PathBuf) {
    let node = pipeline::NodeDef {
        skills: Vec::new(),
        isolated_worktree: Some(isolated),
        id: node_id.into(),
        name: node_id.into(),
        node_type: pipeline::NodeType::Agent,
        inputs: Vec::new(),
        outputs: Vec::new(),
        interactive: false,
        view: None,
        max_iter: None,
        over: None,
        pin_harness: None,
        harnesses: Default::default(),
        agent_choice: None,
        auto_fail: None,
        orchestrator: false,
    };
    let pipeline = pipeline::PipelineDef {
        name: "spawn-unit".into(),
        version: None,
        variables: HashMap::new(),
        nodes: vec![node.clone()],
        edges: Vec::new(),
        loops: Vec::new(),
        notes: Vec::new(),
        prompt_required: false,
        grid_size: None,
    };
    let pipeline_path = repo_root.join("spawn-unit.yaml");
    (pipeline, node, pipeline_path)
}

/// The terminal tails of `node_done`/`node_fail`/`node_skip` are DETACHED
/// (ADR-0023): the response returns before the run has advanced.
/// Tests that drive those handlers must poll to quiescence instead of
/// asserting synchronously after the response.
pub async fn wait_run_status(
    state: &Arc<TestState>,
    run_id: &str,
    want: event_log::RunStatus,
) -> event_log::RunState {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let events = load_events(&state.db, run_id).await.unwrap();
        if let Some(rs) = event_log::project(&events) {
            if rs.status == want {
                return rs;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for run {run_id} to reach {want:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

pub fn write_artifact_for(
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    port: &str,
    content: &str,
) {
    let dir = worktree_dir_for_run(repo_root, run_id)
        .join(".pdo")
        .join("artifacts")
        .join(node_id)
        .join("iter-1")
        .join(port);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), content).unwrap();
}

/// The pipeline the isolated restart twin needs: `worker` owns a
/// sub-worktree, which is the ONLY node class #489 breaks.
pub fn write_isolated_node_pipeline(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

/// Pipeline `worker -> consumer` (both non-isolated, no declared outputs) so
/// downstream re-spawn behavior is observable in the event log.
pub fn write_pipeline_with_consumer(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n  - id: consumer\n    name: consumer\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: feed\n    outputs:\n      - name: out\nedges:\n  - source: {{ node: worker, port: result }}\n    target: {{ node: consumer, port: feed }}\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

pub fn write_pipeline_with_frontmatter(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: review\n        frontmatter:\n          verdict:\n            type: enum\n            allowed: [PASS, FAIL]\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

pub fn write_pipeline_with_outputs(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: summary\n      - name: report\n    view: {{ x: 100, y: 100 }}\nedges: []\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

/// A one-node (`worker`) pipeline written where both the run-scoped and the
/// library resolver look, for the retry handler's `parse_pipeline`.
pub fn write_retry_pipeline(repo_root: &std::path::Path) {
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
}

pub fn write_script_pipeline_with_outputs(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: script\n    inputs:\n      - name: task\n    outputs:\n      - name: out\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}

pub fn write_test_pipeline(dir: &std::path::Path, name: &str) {
    let pipelines_dir = dir.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
    std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
}
