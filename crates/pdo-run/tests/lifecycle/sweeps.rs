//! The daemon's own transitions: the run-level stall, boot recovery, the liveness sweep.

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
async fn reconcile_fails_a_run_with_an_orphaned_ready_node_idle_past_grace() {
    // #279 Layer 2, end-to-end through the periodic reconcile path. Build the
    // exact silent-spawn-abort end-state: `a` completed, `b` is ready with NO
    // event (advance_run never drove it), no live node, and the log has gone
    // idle past the grace window. The pre-#279 reconcile saw `b` as "ready,
    // about to be driven" and left the run Running forever. It must now fail
    // it loud (RunFailed) with a cause naming the undriven node.
    let tmp = tempfile::tempdir().unwrap();
    let pipelines = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines).unwrap();
    std::fs::write(
            pipelines.join("stall-test.yaml"),
            "name: stall-test\nversion: \"1.0\"\nprompt_required: false\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: a\n    name: a\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n  - id: b\n    name: b\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: in\n    outputs:\n      - name: out\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: a, port: task }\n  - source: { node: a, port: result }\n    target: { node: b, port: in }\n  - source: { node: b, port: out }\n    target: { node: end, port: result }\n",
        )
        .unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "20260630-l2-orphan";

    // Backdate every event well past the grace window so the run reads as idle.
    let old_ts = (chrono::Utc::now() - chrono::Duration::seconds(SPAWN_STALL_GRACE_SECS + 180))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    append_event_at(
        &state,
        run_id,
        &old_ts,
        event_log::EventKind::RunStarted,
        None,
        None,
        Some(serde_json::json!({ "pipeline_name": "stall-test" })),
    )
    .await;
    append_event_at(
        &state,
        run_id,
        &old_ts,
        event_log::EventKind::NodeStarted,
        Some("a"),
        Some(1),
        None,
    )
    .await;
    append_event_at(
        &state,
        run_id,
        &old_ts,
        event_log::EventKind::NodeCompleted,
        Some("a"),
        Some(1),
        None,
    )
    .await;

    // Precondition: `b` is the orphan signature — ready, no event, no live node.
    let (events, run_state) = reload_run_state(&state.run, run_id).await.unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Running);
    assert!(
        !run_state.nodes.contains_key("b"),
        "precondition: `b` has no event (silent spawn-abort orphan)"
    );
    let pipeline = pipeline::parse_pipeline(
        &std::fs::read_to_string(pipelines.join("stall-test.yaml")).unwrap(),
    )
    .unwrap()
    .pipeline;
    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    assert_eq!(
        ready.iter().map(|r| r.node_id.as_str()).collect::<Vec<_>>(),
        vec!["b"],
        "precondition: only `b` is ready to spawn"
    );
    assert!(
        max_event_age_secs(&events).is_some_and(|s| s >= SPAWN_STALL_GRACE_SECS),
        "precondition: the run is idle past the grace window"
    );

    // The periodic reconcile path must now park the run loud (ADR-0049:
    // AwaitingUser, never RunFailed).
    reconcile_run_level_stall(&state.run, run_id).await;

    let events = load_events(&state.db, run_id).await.unwrap();
    let run_interrupted = events
        .iter()
        .find(|e| e.kind == event_log::EventKind::RunInterrupted)
        .expect("reconcile must append a RunInterrupted for the wedged run (#279)");
    let reason = run_interrupted
        .payload
        .as_ref()
        .and_then(|p| p.get("reason"))
        .and_then(|r| r.as_str())
        .unwrap_or_default();
    assert!(
        reason.contains("#279): b"),
        "the park cause {reason:?} must name the undriven node `b` and reference #279"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "the runtime never fails a stalled run of its own initiative (ADR-0049)"
    );
    let projected = event_log::project(&events).unwrap();
    assert_eq!(
        projected.status,
        event_log::RunStatus::AwaitingUser,
        "the run must park AwaitingUser, not wedge Running and not fail"
    );
    assert_eq!(projected.awaiting_reason.as_deref(), Some(reason));
}

#[tokio::test]
async fn reconcile_leaves_a_fresh_run_running_within_the_grace_window() {
    // The other side of the grace window: the SAME orphan-shaped state but
    // with fresh (current) timestamps must NOT be failed — `advance_run` is
    // about to drive `b`. Guards against a reconcile that races every healthy
    // just-completed node to a false RunFailed.
    let tmp = tempfile::tempdir().unwrap();
    let pipelines = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines).unwrap();
    std::fs::write(
            pipelines.join("stall-test.yaml"),
            "name: stall-test\nversion: \"1.0\"\nprompt_required: false\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: a\n    name: a\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n  - id: b\n    name: b\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: in\n    outputs:\n      - name: out\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: a, port: task }\n  - source: { node: a, port: result }\n    target: { node: b, port: in }\n  - source: { node: b, port: out }\n    target: { node: end, port: result }\n",
        )
        .unwrap();

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "20260630-l2-fresh";

    seed_run_for_node_control(&state, run_id, "stall-test").await;
    seed_node_started(&state, run_id, "a", 1).await;
    seed_node_completed(&state, run_id, "a", 1).await;

    reconcile_run_level_stall(&state.run, run_id).await;

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunFailed),
        "a freshly-completed node within the grace window must not be reconciled to Failed"
    );
    assert_eq!(
        event_log::project(&events).unwrap().status,
        event_log::RunStatus::Running,
        "the run must remain Running — advance_run is about to drive `b`"
    );
}

/// A terminal run (here `Failed` via fail-fast) that still projects a
/// session-holding node must be reconciled at boot — the dangling node is
/// marked `Failed`, the projection becomes consistent, and the pass is
/// idempotent across restarts.
#[tokio::test]
async fn boot_recovery_reconciles_session_holding_node_in_terminal_run() {
    let tmp = tempfile::tempdir().unwrap();
    let pipe_name = "boot-rec-215";
    write_test_pipeline(tmp.path(), pipe_name);

    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "boot-rec-215-1";
    // RunStarted -> NodeStarted(worker, iter 1) -> RunFailed: a terminal run
    // whose worker node is still projected Running. Run* events bypass the
    // transition guard, so this faithfully reproduces fail-fast leaving a
    // sibling session-holding.
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
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "reason": "fail-fast: sibling failed" })),
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    // Before recovery: the projection still shows worker Running, but the
    // Phase-1 count already excludes it (run is terminal) — the slot no
    // longer leaks.
    let before = event_log::project(&load_events(&state.db, run_id).await.unwrap()).unwrap();
    assert_eq!(before.status, event_log::RunStatus::Failed);
    assert_eq!(
        before.nodes["worker"].status,
        event_log::NodeStatus::Running,
        "precondition: worker still projects Running before recovery"
    );
    assert_eq!(
        admission::count_live_node_sessions([&before]),
        0,
        "Phase 1 already excludes terminal-run nodes from the cap"
    );

    boot_recovery::run_boot_recovery(&state.run).await;

    // After recovery: worker is reconciled to Interrupted (ADR-0049: a
    // session lost to a daemon restart is an infra incident, not a failure)
    // with an explanatory reason, and no terminal run carries a
    // session-holding node.
    let after = event_log::project(&load_events(&state.db, run_id).await.unwrap()).unwrap();
    assert_eq!(
        after.nodes["worker"].status,
        event_log::NodeStatus::Interrupted
    );
    let reason = after.nodes["worker"]
        .failure_reason
        .as_deref()
        .unwrap_or_default();
    assert!(
        reason.contains("terminal") && reason.contains("session-holding"),
        "reason should explain the terminal/session-holding reconciliation, got: {reason}"
    );
    assert!(
        !after.status.is_live()
            && !after.nodes.values().any(|ns| {
                matches!(
                    ns.status,
                    event_log::NodeStatus::Running | event_log::NodeStatus::AwaitingUser
                )
            }),
        "no terminal run may carry a Running/AwaitingUser node after recovery"
    );

    // Idempotency: a second pass (e.g. another reboot) appends no duplicate
    // NodeInterrupted — validate_interrupt returns NoOp on the already-interrupted iter.
    boot_recovery::run_boot_recovery(&state.run).await;
    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeInterrupted, "worker"),
        1,
        "boot recovery must be idempotent: exactly one NodeInterrupted for worker"
    );
    let again = event_log::project(&events).unwrap();
    assert_eq!(
        again.nodes["worker"].status,
        event_log::NodeStatus::Interrupted
    );
}

#[tokio::test]
async fn stale_detector_terminal_events_on_terminal_node_are_dropped() {
    // The liveness sweep probes a snapshot; if the node completed
    // organically in between, its late `NodeFailed` must be dropped by the
    // guard at append time (re-checked against the freshly projected state).
    //
    // Since #469 the only terminal event the sweep builds itself is the
    // session-died `NodeFailed` — the turn-end path goes through the shared
    // node-completion body, whose own guard head refuses a completed
    // iteration (see `completion_head_noops_duplicate_completion`).
    let state = test_state().await;
    let run_id = "guard-stale-terminal";
    for event in [
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
        seed_event(
            run_id,
            event_log::EventKind::NodeCompleted,
            Some("worker"),
            Some(1),
            None,
        ),
    ] {
        append_event(&state, &event).await.unwrap();
    }

    for detection in [
        stale_detector::Detection::SessionDied,
        stale_detector::Detection::TurnEnded,
        stale_detector::Detection::Ok,
    ] {
        for event in stale_detector::detection_events(&detection, run_id, "worker", 1) {
            // The guard turns these into no-ops, not errors.
            append_event(&state, &event).await.unwrap();
        }
    }

    let events = load_events(&state.db, run_id).await.unwrap();
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeStale, "worker"),
        0,
        "#469: nothing in the daemon emits NodeStale any more"
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeFailed, "worker"),
        0,
        "a late session-died failure must be dropped on an already-completed node"
    );
    assert_eq!(
        count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
        1,
        "no duplicate completion"
    );
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(
        run_state.nodes["worker"].status,
        event_log::NodeStatus::Completed
    );
}
