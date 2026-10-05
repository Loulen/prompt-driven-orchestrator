//! The spawn of a node: the Start button, `start_node`, the direct drive of `spawn_node` and its guards.

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
async fn node_start_returns_conflict_when_already_running() {
    let state = test_state().await;
    seed_run_for_node_control(&state, "start-conflict", "test-pipe").await;
    seed_node_started(&state, "start-conflict", "worker", 1).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/start-conflict/nodes/worker/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn node_start_returns_404_for_unknown_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/nodes/worker/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_node_command_force_spawns_pending_node() {
    // Happy path: the `start_node` /commands kind force-spawns a pending
    // worker on a live run, returns 200, and lands a NodeStarted event
    // (proving the spawn went through `force_spawn_node`).
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "test-pipe");
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "start-cmd-ok";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "start_node", "node_id": "worker" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .into_owned();
    assert_eq!(
        status,
        StatusCode::OK,
        "force-spawn returned {status}: {body}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    // The audit CommandIssued event landed...
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued
                && e.payload
                    .as_ref()
                    .and_then(|p| p.get("command"))
                    .and_then(|c| c.as_str())
                    == Some("start_node")),
        "expected a start_node CommandIssued audit event"
    );
    // ...and so did the NodeStarted the force-spawn produced.
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "expected a NodeStarted event for the force-spawned worker"
    );
}

#[tokio::test]
async fn start_node_command_requires_node_id() {
    let state = test_state().await;
    let run_id = "start-cmd-no-node";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/commands"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "start_node" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn force_spawn_on_terminal_run_embeds_reopen() {
    // AC7 / ADR-0049 (amends #204/#487): the UI Start button on a terminal Run
    // now EMBEDS the re-open — the human's gesture re-opens the Run atomically
    // and drives the node, instead of the pre-résilience 409. The re-open is
    // the human's, never the runtime's (ADR-0009 amended).
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "test-pipe");
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "start-terminal";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;
    seed_run_terminal(&state, run_id, event_log::EventKind::RunCompleted).await;

    let app = build_router(state.clone());
    let _ = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Regardless of whether the real tmux spawn succeeds in the test env, the
    // re-open gesture is embedded and the Run is live again — the previous
    // terminal `RunCompleted` label is preserved (AC12).
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::CommandIssued
                && e.payload.as_ref().and_then(|p| p.get("command"))
                    == Some(&serde_json::json!("reopen_run"))),
        "the Start must embed a reopen_run CommandIssued on a terminal run"
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunCompleted),
        "the previous terminal label stays in the log (AC12)"
    );
    let projected = event_log::project(&events).unwrap();
    assert!(
        projected.status.is_live(),
        "the Run is live again after the embedded re-open, got {:?}",
        projected.status
    );
}

#[tokio::test]
async fn force_spawn_at_session_cap_is_rejected() {
    // D5: force-spawn must fail fast with 409 when the global session
    // cap is already reached, rather than push past it toward tmux-server
    // collapse. Seed `DEFAULT_SESSION_CAP` live sessions in a
    // filler run so the assertion holds for any configured cap ≤ default
    // (race-free vs the env-var cap test — no global state touched).
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "test-pipe");
    let state = test_state_with_dir(tmp.path()).await;

    let filler = "cap-filler";
    seed_run_for_node_control(&state, filler, "test-pipe").await;
    for i in 0..admission::DEFAULT_SESSION_CAP {
        seed_node_started(&state, filler, &format!("filler-{i}"), 1).await;
    }
    assert_eq!(
        count_global_live_sessions(&state.run).await,
        admission::DEFAULT_SESSION_CAP
    );

    let run_id = "cap-target";
    seed_run_for_node_control(&state, run_id, "test-pipe").await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "session cap reached");

    // The target's worker must not have started.
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted),
        "no NodeStarted may be appended when force-spawn is rejected at the cap"
    );
}

/// INV-3: at the daemon-wide session cap, `spawn_node` throttles the
/// node into `waiting` (appending `NodeWaiting`) instead of starting it — no
/// NodeStarted, no session. Pure in-memory (no git, no tmux).
#[tokio::test]
async fn spawn_node_at_cap_throttles_into_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    // Saturate the global cap with live sessions in a filler run.
    saturate_session_cap(&state).await;
    assert_eq!(
        count_global_live_sessions(&state.run).await,
        admission::DEFAULT_SESSION_CAP
    );

    let run_id = "spawn-unit-throttle";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    let (pipeline, node, pipeline_path) = spawn_test_fixture(tmp.path(), "worker", false);
    let artifacts_dir = tmp.path().join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: tmp.path(),
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: tmp.path(),
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Throttled),
        "spawn at the session cap must throttle into waiting, got {outcome:?}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeWaiting
                && e.node_id.as_deref() == Some("worker")),
        "a throttled spawn must append NodeWaiting for the node"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "a throttled node must not be started"
    );
}

/// INV-4/5/6: when the spawn window panics *after* the sub-worktree is
/// created but *before* NodeStarted, `spawn_node` reaps the orphan
/// (dir + branch) and fails the run LOUD as `RunFailed` — never `NodeFailed`
/// (which the guard would no-op for a never-started node, wedging the run).
/// A direct-call re-cut of `tests/spawn_abort_recovery.rs`.
#[tokio::test]
async fn spawn_node_panic_reaps_orphan_and_fails_run_loud() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let state = test_state_with_dir(repo).await;

    let run_id = "spawn-unit-panic";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    // Create the pipeline branch `pdo/run-<id>` (via the pipeline worktree)
    // so `create_sub_worktree` has a base to branch the sub-worktree from —
    // the same layout the daemon builds at run start.
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    // Arm the one-shot spawn poison: the isolated spawn will create its
    // sub-worktree, then panic at the span head (before NodeStarted).
    state.run.arm_spawn_panic();

    let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", true);
    let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &wt_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Failed { .. }),
        "a spawn that panics before start must return Failed, got {outcome:?}"
    );

    // The orphaned sub-worktree dir + branch are reaped.
    let sub_wt = sub_worktree_path(repo, run_id, "worker", 1);
    assert!(
        !sub_wt.exists(),
        "orphaned sub-worktree {} must be reaped after the aborted spawn",
        sub_wt.display()
    );
    let sub_branch = sub_worktree_branch(run_id, "worker", 1);
    let branch_list = std::process::Command::new("git")
        .args(["branch", "--list", &sub_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_list.stdout)
            .trim()
            .is_empty(),
        "orphaned branch {sub_branch} must be deleted after the aborted spawn"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    // ADR-0049/0050: a spawn abort before start surfaces as a
    // `NodeInterrupted` naming the node (never `RunFailed`), which parks the
    // run `AwaitingUser` — visible and recoverable, never frozen `running`.
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeInterrupted
                && e.node_id.as_deref() == Some("worker")),
        "the spawn abort must surface as a NodeInterrupted naming the node"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a spawn abort (ADR-0049)"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "no NodeStarted may exist for a spawn that aborted before start"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeFailed),
        "the abort must NOT be a NodeFailed (infra death is Interrupted, ADR-0049)"
    );
    // The run projects `AwaitingUser` with the incident reason.
    let rs = event_log::project(&events).unwrap();
    assert_eq!(rs.status, event_log::RunStatus::AwaitingUser);
    assert!(rs.awaiting_reason.is_some());
}

/// #508 (core, no sub-worktree): when the tmux spawn itself fails *after*
/// `NodeStarted` is durable, `spawn_node` no longer swallows the `Err` and
/// returns a lying `Spawned`. It appends `NodeFailed` (legal — the iteration
/// is `Running`) THEN `RunFailed`, moving both node and run terminal with the
/// TRUE cause (a tmux spawn failure), never the false `session_died` the
/// liveness sweep would have written ~30s later. A non-isolated node owns no
/// sub-worktree, so nothing is reaped.
#[tokio::test]
async fn spawn_node_tmux_err_fails_run_loud_no_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let state = test_state_with_dir(repo).await;

    let run_id = "spawn-unit-tmuxerr-noworktree";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    // Plant the collision file directly in the non-isolated node's working dir
    // (= `worktree_dir`); no commit needed since it runs there in place.
    plant_pdo_prompts_file(repo, false);

    let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", false);
    let artifacts_dir = repo.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: repo,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Failed { .. }),
        "a failed tmux spawn must return Failed, not a lying Spawned, got {outcome:?}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    // The reservation was durably recorded (this is the *after-start* path)...
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "the after-start path keeps its NodeStarted (the reservation was durable)"
    );
    // ...and the node is interrupted (ADR-0049: an infra spawn failure is
    // `NodeInterrupted`, not `NodeFailed`), never a `RunFailed`.
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeInterrupted
                && e.node_id.as_deref() == Some("worker")),
        "a failed tmux spawn on a Running iteration must append NodeInterrupted"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a tmux spawn failure (ADR-0049)"
    );

    let (_, run_state) = reload_run_state(&state.run, run_id).await.unwrap();
    assert_eq!(
        run_state.status,
        event_log::RunStatus::AwaitingUser,
        "an interrupted node parks the run AwaitingUser (ADR-0049)"
    );

    let reason = event_reason(&state, run_id, event_log::EventKind::NodeInterrupted).await;
    assert!(
        reason.contains("failed to spawn tmux session"),
        "the interrupt cause must name the tmux spawn failure, got {reason:?}"
    );
    assert!(
        !reason.contains("session_died"),
        "the interrupt cause must NOT be the false session_died, got {reason:?}"
    );
}

/// #508 (fresh sub-worktree → reap): an isolated node whose fresh
/// sub-worktree this spawn created reaps that orphan (dir + branch) on a tmux
/// spawn failure, exactly as the pre-start abort does — the run goes terminal
/// with no leaked worktree.
#[tokio::test]
async fn spawn_node_tmux_err_reaps_fresh_sub_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    // Commit the collision file so the sub-worktree checkout carries it.
    plant_pdo_prompts_file(repo, true);
    let state = test_state_with_dir(repo).await;

    let run_id = "spawn-unit-tmuxerr-reap";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", true);
    let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &wt_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Failed { .. }),
        "a failed tmux spawn must return Failed, got {outcome:?}"
    );

    // The orphaned sub-worktree dir + branch this spawn created are reaped.
    let sub_wt = sub_worktree_path(repo, run_id, "worker", 1);
    assert!(
        !sub_wt.exists(),
        "the fresh sub-worktree {} this spawn created must be reaped",
        sub_wt.display()
    );
    let sub_branch = sub_worktree_branch(run_id, "worker", 1);
    let branch_list = std::process::Command::new("git")
        .args(["branch", "--list", &sub_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_list.stdout)
            .trim()
            .is_empty(),
        "the fresh sub-worktree branch {sub_branch} must be deleted after the reap"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "the after-start path keeps its NodeStarted"
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeInterrupted
                && e.node_id.as_deref() == Some("worker")),
        "a failed tmux spawn on a Running iteration must append NodeInterrupted"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a tmux spawn failure (ADR-0049)"
    );
    let (_, run_state) = reload_run_state(&state.run, run_id).await.unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::AwaitingUser);
    let reason = event_reason(&state, run_id, event_log::EventKind::NodeInterrupted).await;
    assert!(
        reason.contains("failed to spawn tmux session") && !reason.contains("session_died"),
        "interrupt cause must be the true tmux spawn failure, got {reason:?}"
    );
}

/// #508 (reused sub-worktree → NO reap): the critical leg. When the
/// sub-worktree already existed (classified `Reusable`, `created=false`), a
/// tmux spawn failure must destroy NOTHING — the reused work is preserved.
/// `orphan_to_reap` is `None`, so the after-start failure appends
/// `NodeFailed` + `RunFailed` but leaves the sub-worktree and its branch
/// intact (ADR-0037 §6). Inverse of the fresh-worktree case.
#[tokio::test]
async fn spawn_node_tmux_err_preserves_reused_sub_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    plant_pdo_prompts_file(repo, true);
    let state = test_state_with_dir(repo).await;

    let run_id = "spawn-unit-tmuxerr-reuse";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    // Pre-create the sub-worktree so `ensure_sub_worktree` classifies it
    // `Reusable` → `created=false` → `orphan_to_reap = None`.
    let sub_wt = sub_worktree_path(repo, run_id, "worker", 1);
    let sub_branch = sub_worktree_branch(run_id, "worker", 1);
    create_sub_worktree(repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
    assert!(
        sub_wt.exists(),
        "precondition: the reused sub-worktree exists"
    );

    let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", true);
    let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &wt_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Failed { .. }),
        "a failed tmux spawn must return Failed, got {outcome:?}"
    );

    // The run parks AwaitingUser loud (ADR-0049)...
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeInterrupted
                && e.node_id.as_deref() == Some("worker")),
        "a failed tmux spawn on a Running iteration must append NodeInterrupted"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails the run on a tmux spawn failure (ADR-0049)"
    );
    let (_, run_state) = reload_run_state(&state.run, run_id).await.unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::AwaitingUser);

    // ...but the REUSED sub-worktree and its branch are UNTOUCHED (the whole
    // point of the gate: a reuse loses nothing).
    assert!(
        sub_wt.exists(),
        "the reused sub-worktree {} must NOT be reaped — a reuse loses nothing",
        sub_wt.display()
    );
    let branch_list = std::process::Command::new("git")
        .args(["branch", "--list", &sub_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&branch_list.stdout)
            .trim()
            .is_empty(),
        "the reused sub-worktree branch {sub_branch} must still be listed"
    );
}

/// INV-1: the transition guard refuses an illegal NodeStarted BEFORE
/// any side effect. Spawning iter-2 of a node whose iter-1 is still live is
/// rejected — `Refused`, and no NodeStarted for iter-2 is appended.
#[tokio::test]
async fn spawn_node_refuses_concurrent_iteration() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;

    let run_id = "spawn-unit-refuse";
    // Seeds RunStarted + NodeStarted worker iter-1 (iter-1 is live).
    seed_run_with_node(&state, run_id, "worker", false).await;

    let (pipeline, node, pipeline_path) = spawn_test_fixture(tmp.path(), "worker", false);
    let artifacts_dir = tmp.path().join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: tmp.path(),
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: tmp.path(),
    };

    // Spawn iter-2 while iter-1 is live → guard rejects.
    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 2).await;
    assert!(
        matches!(outcome, SpawnOutcome::Refused { .. }),
        "spawning a concurrent iteration must be Refused, got {outcome:?}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted && e.iter == Some(2)),
        "a refused spawn must not append NodeStarted for iter-2"
    );
    let iter1_starts = events
        .iter()
        .filter(|e| {
            e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")
                && e.iter == Some(1)
        })
        .count();
    assert_eq!(
        iter1_starts, 1,
        "the live iter-1 NodeStarted must be untouched by the refused iter-2 spawn"
    );
}

/// AC "blocage", at the chokepoint: a sandboxed Run mid-prep defers the spawn
/// before ANY side effect — no `NodeStarted` (which would be the lie the stale
/// detector later reports as `session_died`) and, just as important, no
/// `NodeWaiting` either: a `Waiting` node leaves `compute_ready_to_spawn`, so the
/// reservation would move the replay onto the cross-run admission sweep and a Run
/// whose only trigger was the watcher could wedge for ever.
#[tokio::test]
async fn spawn_node_defers_while_sandbox_prep_is_pending() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;

    let run_id = "spawn-unit-sbx-pending";
    seed_sandboxed_run_mid_prep(&state, run_id, "spawn-unit").await;

    let (pipeline, node, pipeline_path) = spawn_test_fixture(tmp.path(), "worker", false);
    let artifacts_dir = tmp.path().join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: tmp.path(),
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: tmp.path(),
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    let SpawnOutcome::Deferred { reason } = outcome else {
        panic!("a spawn into a Run whose container is not up must be Deferred, got {outcome:?}");
    };
    assert!(
        reason.contains("sandbox prep") && reason.contains("full"),
        "the deferral must name the sandbox prep and the profile, got {reason:?}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted),
        "a deferred spawn must append NO NodeStarted — that event is what makes the \
             stale detector report session_died 25s later"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeWaiting),
        "a deferred spawn must reserve NOTHING: a Waiting node leaves \
             compute_ready_to_spawn and would never be replayed by advance_run"
    );
}

/// The precondition must not leak onto the host path: an `off` Run has no prep
/// events at all, and `sandbox_prep = None` must not be read as "not ready" for
/// it — that would deadlock every non-sandboxed Run in the instance.
#[tokio::test]
async fn spawn_node_never_defers_a_host_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;

    let run_id = "spawn-unit-host";
    // RunStarted with no `sandbox` key at all → SandboxMode::Off, prep = None.
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;

    let (pipeline, node, pipeline_path) = spawn_test_fixture(tmp.path(), "worker", false);
    let artifacts_dir = tmp.path().join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: tmp.path(),
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: tmp.path(),
    };

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Spawned { .. }),
        "an `off` Run must spawn exactly as before the precondition, got {outcome:?}"
    );
}

/// #653 / ADR-0060 — **placement**: an Agent works where its
/// `isolated_worktree` says, and the spawn FREEZES that answer onto
/// `NodeStarted`. Both values, one real git repo.
#[tokio::test]
async fn spawn_places_an_agent_by_its_isolation_and_freezes_it() {
    for isolated in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);
        let state = test_state_with_dir(repo).await;

        let run_id = if isolated { "iso-yes" } else { "iso-no" };
        seed_run_for_node_control(&state, run_id, "spawn-unit").await;
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        create_worktree(repo, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();

        let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", isolated);
        let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
        let resolved_vars = HashMap::new();
        let ctx = SpawnContext {
            pipeline: &pipeline,
            run_id,
            pipeline_path: &pipeline_path,
            worktree_dir: &wt_dir,
            artifacts_dir: &artifacts_dir,
            resolved_vars: &resolved_vars,
            repo_root: repo,
        };
        let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
        assert!(
            matches!(outcome, SpawnOutcome::Spawned { .. }),
            "isolated={isolated}: spawn must start, got {outcome:?}"
        );

        // The sub-worktree exists iff the node asked for one.
        let sub_wt = sub_worktree_path(repo, run_id, "worker", 1);
        assert_eq!(
            sub_wt.exists(),
            isolated,
            "isolated={isolated}: sub-worktree presence must follow the node's own line"
        );

        // …and the answer is frozen on the start event, not re-derived later.
        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            merge_action::frozen_isolation(&events, "worker", 1),
            Some(isolated),
            "isolated={isolated}: NodeStarted must record the isolation it launched with"
        );
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes["worker"].isolated_worktree,
            Some(isolated),
            "isolated={isolated}: the projection must expose the frozen value"
        );
    }
}

/// ADR-0050 §2 — a dispatch that finds its iteration already started is the
/// benign loser, refused before any side effect: no second `NodeStarted`. The
/// window it closes: a first dispatch's `NodeStarted` is durable before its tmux
/// session exists, so the session probe alone let a racing second dispatch (the
/// pipeline watcher's `advance_run` beside a completion's spawn) through.
#[tokio::test]
async fn a_second_dispatch_of_a_started_iteration_is_refused_before_any_side_effect() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let state = test_state_with_dir(repo).await;

    let run_id = "dispatch-twice";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    create_worktree(repo, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();
    let (pipeline, node, pipeline_path) = spawn_test_fixture(repo, "worker", false);
    let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &wt_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };
    spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    let started = |events: &[event_log::Event]| {
        events
            .iter()
            .filter(|e| e.kind == event_log::EventKind::NodeStarted)
            .count()
    };
    assert_eq!(started(&load_events(&state.db, run_id).await.unwrap()), 1);

    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &node, 1).await;
    match outcome {
        SpawnOutcome::Refused { reason } => assert!(reason.contains("already started"), "{reason}"),
        other => panic!("a second dispatch must be refused, got {other:?}"),
    }
    assert_eq!(
        started(&load_events(&state.db, run_id).await.unwrap()),
        1,
        "the loser appends no NodeStarted"
    );

    // The restart of that iteration still re-spawns it.
    let restart = SpawnDeps::from_state(&state.run).replacing_live_iteration();
    let outcome = spawn_node(restart, &ctx, &node, 1).await;
    assert!(
        matches!(outcome, SpawnOutcome::Spawned { .. }),
        "{outcome:?}"
    );
}

/// #653 / ADR-0060 — **the freeze bites**: once a NodeRun has started, a
/// document that now says the opposite does not move it. A re-spawn of the
/// same iteration reads the frozen answer, so recovery lands back in the
/// working directory the interrupted attempt left.
#[tokio::test]
async fn a_live_iteration_keeps_its_frozen_working_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let state = test_state_with_dir(repo).await;

    let run_id = "iso-freeze";
    seed_run_for_node_control(&state, run_id, "spawn-unit").await;
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    create_worktree(repo, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();

    // First spawn: shared worktree.
    let (pipeline, shared_node, pipeline_path) = spawn_test_fixture(repo, "worker", false);
    let artifacts_dir = wt_dir.join(".pdo").join("artifacts");
    let resolved_vars = HashMap::new();
    let ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &wt_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: repo,
    };
    spawn_node(SpawnDeps::from_state(&state.run), &ctx, &shared_node, 1).await;

    // The author now flips the node to isolated and the SAME iteration is
    // re-spawned (a restart). It must stay in the Run worktree.
    let mut isolated_node = shared_node.clone();
    isolated_node.isolated_worktree = Some(true);
    let restart = SpawnDeps::from_state(&state.run).replacing_live_iteration();
    spawn_node(restart, &ctx, &isolated_node, 1).await;

    assert!(
        !sub_worktree_path(repo, run_id, "worker", 1).exists(),
        "an isolation edit must not fork a sub-worktree under a live iteration"
    );
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        merge_action::frozen_isolation(&events, "worker", 1),
        Some(false),
        "the re-spawn must carry the ORIGINAL frozen isolation forward"
    );

    // The next iteration is a new NodeRun and reads the document afresh.
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
    let outcome = spawn_node(SpawnDeps::from_state(&state.run), &ctx, &isolated_node, 2).await;
    assert!(
        matches!(outcome, SpawnOutcome::Spawned { .. }),
        "iter 2 must spawn, got {outcome:?}"
    );
    assert_eq!(
        merge_action::frozen_isolation(&load_events(&state.db, run_id).await.unwrap(), "worker", 2),
        Some(true),
        "a fresh iteration must pick up the edited isolation"
    );
}

/// Both ACs on the real advance path, in order — this is the regression test for
/// the reported failure and for the deadlock a naive fix would introduce.
///
/// 1. **Blocage**: `advance_run` starts nothing while the prep is pending.
/// 2. **Rejeu**: `mark_sandbox_prep_ready` + the following `advance_run` — the
///    exact pair the create-time prep task runs — starts the deferred node.
///    Without this half a Run whose only trigger was the watcher stays `Running`
///    with nothing live for ever, strictly worse than the bug being fixed.
#[tokio::test]
async fn sandbox_pending_run_is_not_schedulable_until_prep_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    let state = test_state_with_dir(repo_root).await;

    let run_id = "sbx-advance-replay";
    // The default `prompt_required: true` keeps the spawn off the `_input`
    // artifact read; `worker` has no incoming edge, so it is a root the readiness
    // sweep reports ready from the very first tick.
    let yaml = format!(
            "name: sbx-advance\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
    let yaml = yaml.as_str();
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("pipeline.yaml"), yaml).unwrap();

    seed_sandboxed_run_mid_prep(&state, run_id, "sbx-advance").await;

    // (1) The watcher's tick, mid-prep: nothing may start.
    spawn_ready_after_event(&state.run, run_id).await;
    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted),
        "the watcher-driven advance must start no node while the container is \
             still being prepared"
    );

    // The node kept NO state, so the scheduler still sees it as ready — that is
    // what makes the replay below possible at all.
    let run_state = event_log::project(&events).unwrap();
    let parsed = pipeline::parse_pipeline(yaml).unwrap().pipeline;
    assert_eq!(
        scheduler_dispatcher::compute_ready_to_spawn(&parsed, &run_state)
            .into_iter()
            .map(|r| r.node_id)
            .collect::<Vec<_>>(),
        vec!["worker".to_string()],
        "a deferred node must stay in the ready set, or nothing could replay it"
    );

    // (2) Prep finishes: the create-time pair replays the deferred spawn.
    mark_sandbox_prep_ready(&state.run, run_id).await;
    spawn_ready_after_event(&state.run, run_id).await;

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted
                && e.node_id.as_deref() == Some("worker")),
        "the spawn deferred during the prep MUST be replayed once \
             sandbox_prep_ready lands — otherwise the run wedges for ever"
    );
}

/// The force-spawn path (`POST …/nodes/<id>/start`, and the manager's
/// `start_node`) does not route through `spawn_node`: it drives
/// `node_primitives::start_node`, so it states the precondition itself. 409 with a
/// reason, not a silent queue — "start now" must not defer, and the operator who
/// pressed the button gets told why.
#[tokio::test]
async fn force_spawn_is_refused_while_sandbox_prep_is_pending() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_root = tmp.path();
    write_test_pipeline(repo_root, "sbx-force");
    let state = test_state_with_dir(repo_root).await;

    let run_id = "sbx-force-spawn";
    seed_sandboxed_run_mid_prep(&state, run_id, "sbx-force").await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/nodes/worker/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("sandbox prep"),
        "the 409 must explain that the sandbox is still being prepared, got {json}"
    );

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStarted),
        "a refused force-spawn must start nothing"
    );
}
