//! The Runs as a list (`GET /runs`) and their reapable part (`GET /runs/reapable`),
//! moved from the legacy crate with the code they test (#925).

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
async fn list_runs_empty() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(runs.is_empty());
}

// GET /runs/reapable (#128 Track A)
//
// Read-only surfacing of terminal runs whose on-disk worktree(s) still
// exist, so a human or a janitor pipeline can reclaim the disk via the
// explicit `cleanup_run` command. The runtime NEVER deletes here
// (ADR-0012(a)); `reaper_never_deletes_worktree` below is the executable
// pin of that doctrine.

/// Seed a run as `RunStarted` followed by `extra` run-level events (e.g.
/// `RunCompleted`, `RunFailed`, `RunArchived`), with no nodes spawned. Lets
/// the `/runs/reapable` tests construct a run in any terminal/live status.
async fn seed_run_with_run_events(
    state: &Arc<TestState>,
    run_id: &str,
    pipeline_name: &str,
    extra: &[event_log::EventKind],
) {
    let mut events = vec![event_log::Event {
        id: None,
        run_id: run_id.into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
    }];
    for kind in extra {
        events.push(event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: kind.clone(),
            node_id: None,
            iter: None,
            payload: None,
        });
    }
    for ev in &events {
        append_event(state, ev).await.unwrap();
    }
}

/// Create the on-disk worktree dir the daemon would have forked for a run,
/// so the `/runs/reapable` `worktree_present` check sees it.
fn mk_run_worktree_dir(repo_root: &std::path::Path, run_id: &str) {
    std::fs::create_dir_all(
        repo_root
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree"),
    )
    .unwrap();
}

async fn get_reapable(state: &Arc<TestState>, query: &str) -> Vec<serde_json::Value> {
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/reapable{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /runs/reapable should 200"
    );
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn reapable_lists_terminal_run_with_worktree_present() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reap-completed";
    seed_run_with_run_events(
        &state,
        run_id,
        "test-pipe",
        &[event_log::EventKind::RunCompleted],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), run_id);

    let runs = get_reapable(&state, "").await;
    assert_eq!(
        runs.len(),
        1,
        "expected the completed worktree-present run: {runs:?}"
    );
    let e = &runs[0];
    assert_eq!(e["run_id"], run_id);
    assert_eq!(e["pipeline_name"], "test-pipe");
    assert_eq!(e["status"], "completed");
    assert_eq!(e["worktree_present"], true);
    assert!(
        e["completed_at"].is_string(),
        "completed_at must be surfaced"
    );
    assert!(
        e["age_secs"].as_i64().map(|a| a >= 0).unwrap_or(false),
        "age_secs must be a non-negative number: {:?}",
        e.get("age_secs")
    );
    // No size requested → the field is omitted (default stays fast).
    assert!(
        e.get("approx_disk_bytes").is_none() || e["approx_disk_bytes"].is_null(),
        "approx_disk_bytes must be absent without ?size=true"
    );
}

/// End-to-end janitor loop: a real `/runs/reapable?size=true`
/// response parses into `reap_policy::ReapableRun`, the pure policy selects
/// the terminal run, and `cleanup_run` archives it so a re-list is empty.
/// Ties the two independently-tested halves — endpoint shape and pure policy
/// — to the `cleanup_run` contract `pdo reap` depends on. Leak-free (all
/// state is an in-memory DB + a tempdir).
#[tokio::test]
async fn reap_policy_selects_and_cleanup_removes_a_real_reapable_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reap-e2e-completed";
    seed_run_with_run_events(
        &state,
        run_id,
        "some-pipe",
        &[event_log::EventKind::RunCompleted],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), run_id);

    // The real endpoint payload (with sizes) deserializes into the policy's
    // own type — no hand-written fixture.
    let raw = get_reapable(&state, "?size=true").await;
    let runs: Vec<reap_policy::ReapableRun> =
        serde_json::from_value(serde_json::Value::Array(raw)).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, run_id);

    // A freshly-seeded run has age ≈ 0; a 0-second completed TTL selects it
    // (the graded default TTLs are pinned by the `reap_policy` unit tests).
    let policy = reap_policy::ReapPolicy {
        completed_ttl_secs: 0,
        self_pipeline: None,
        ..Default::default()
    };
    let plan = policy.plan(&runs);
    assert_eq!(
        plan.reclaim.len(),
        1,
        "the completed worktree-present run must be selected"
    );
    assert_eq!(plan.reclaim[0].run_id, run_id);

    // Reclaim it and confirm the surface no longer lists it (archived + the
    // run dir removed) — the honest proof `pdo reap` re-lists for.
    let resp = cleanup_run(&state, run_id).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let after = get_reapable(&state, "?size=true").await;
    assert!(
        after.is_empty(),
        "a reclaimed run must no longer be reapable: {after:?}"
    );
}

#[tokio::test]
async fn reapable_excludes_live_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    // Running (RunStarted only) and Paused are both live — never reapable,
    // even with a worktree on disk.
    seed_run_with_run_events(&state, "reap-running", "p", &[]).await;
    mk_run_worktree_dir(tmp.path(), "reap-running");
    seed_run_with_run_events(
        &state,
        "reap-paused",
        "p",
        &[event_log::EventKind::RunPaused],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), "reap-paused");

    let runs = get_reapable(&state, "").await;
    assert!(
        runs.is_empty(),
        "live runs must never be reapable: {runs:?}"
    );
}

#[tokio::test]
async fn reapable_excludes_archived_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reap-archived";
    seed_run_with_run_events(
        &state,
        run_id,
        "p",
        &[
            event_log::EventKind::RunCompleted,
            event_log::EventKind::RunArchived,
        ],
    )
    .await;
    // Even if a stray worktree dir lingers, an archived run never surfaces
    // (its disk was already reclaimed by cleanup_run).
    mk_run_worktree_dir(tmp.path(), run_id);

    let runs = get_reapable(&state, "").await;
    assert!(
        runs.is_empty(),
        "archived runs are already reclaimed: {runs:?}"
    );
}

#[tokio::test]
async fn reapable_reflects_worktree_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reap-gone";
    seed_run_with_run_events(&state, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
    mk_run_worktree_dir(tmp.path(), run_id);
    assert_eq!(
        get_reapable(&state, "").await.len(),
        1,
        "present worktree → listed"
    );

    // Reclaim the disk (what cleanup_run does to the worktree) → drops off.
    std::fs::remove_dir_all(tmp.path().join(".pdo").join("runs").join(run_id)).unwrap();
    assert!(
        get_reapable(&state, "").await.is_empty(),
        "gone worktree → de-listed"
    );
}

#[tokio::test]
async fn reapable_tags_terminal_status_for_consumer_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    // Surfacing is read-only: Failed/Halted/Skipped all appear, each tagged
    // with its status, so the consumer (janitor recipe) applies its own
    // policy (e.g. completed-only). We do NOT pre-filter failures here.
    seed_run_with_run_events(
        &state,
        "reap-failed",
        "p",
        &[event_log::EventKind::RunFailed],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), "reap-failed");
    seed_run_with_run_events(
        &state,
        "reap-halted",
        "p",
        &[event_log::EventKind::RunHalted],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), "reap-halted");
    seed_run_with_run_events(
        &state,
        "reap-skipped",
        "p",
        &[event_log::EventKind::RunSkipped],
    )
    .await;
    mk_run_worktree_dir(tmp.path(), "reap-skipped");

    let runs = get_reapable(&state, "").await;
    let by_id: std::collections::HashMap<String, String> = runs
        .iter()
        .map(|e| {
            (
                e["run_id"].as_str().unwrap().to_string(),
                e["status"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(by_id.get("reap-failed").map(String::as_str), Some("failed"));
    assert_eq!(by_id.get("reap-halted").map(String::as_str), Some("halted"));
    assert_eq!(
        by_id.get("reap-skipped").map(String::as_str),
        Some("skipped")
    );
    assert_eq!(
        runs.len(),
        3,
        "all three terminal-non-archived runs surface"
    );
}

#[tokio::test]
async fn reapable_size_query_reports_disk_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    let run_id = "reap-size";
    seed_run_with_run_events(&state, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
    mk_run_worktree_dir(tmp.path(), run_id);
    std::fs::write(
        tmp.path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree")
            .join("blob.bin"),
        vec![0u8; 4096],
    )
    .unwrap();

    // ?size=true → a numeric size reflecting the on-disk bytes.
    let runs = get_reapable(&state, "?size=true").await;
    assert_eq!(runs.len(), 1);
    let bytes = runs[0]["approx_disk_bytes"].as_u64();
    assert!(
        bytes.map(|b| b >= 4096).unwrap_or(false),
        "expected approx_disk_bytes >= 4096, got {:?}",
        runs[0].get("approx_disk_bytes")
    );
}

#[tokio::test]
async fn halted_run_appears_in_list_with_correct_status() {
    let state = test_state().await;
    let run_id = "halt-test-1";

    // Seed: run_started → node_started → node_completed → run_halted
    for event in [
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "halt-pipe" })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("reviewer".into()),
            iter: Some(3),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some("reviewer".into()),
            iter: Some(3),
            payload: None,
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunHalted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "message": "Blocked after 3 iterations" })),
        },
    ] {
        append_event(&state, &event).await.unwrap();
    }

    // Verify run state is Halted
    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    assert_eq!(run_state.status, event_log::RunStatus::Halted);

    // Verify it appears in the list
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
    assert_eq!(runs[0]["status"], "halted");
}
