//! Every Pipeline Manager command on an unstarted and on a forgotten Run,
//! archive included — moved from the legacy crate with the code they test
//! (#925).

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

const JSON_CT: &str = "application/json";
// Every `/commands` error body is JSON now — no text/plain constant
// remains, which is itself the invariant (a text body would need re-introducing
// the constant, a visible diff).

#[tokio::test]
async fn every_kind_status_and_content_type_on_an_unstarted_run() {
    // Against a run with an empty event log this surface answers in two shapes:
    //   * four kinds answer 200 and APPEND — they are designed to work before the
    //     run exists (`inject_artifact` writes the file the run has not yet
    //     produced; `rename_run` appends blind);
    //   * the rest answer 404, all in JSON `{"error":"run not found"}`.
    //
    // Hoisting the `load_events -> project` preamble to the top of the handler —
    // the single most tempting refactor here — flips the first group to 404, and
    // no other test in the repo would notice.
    //
    // One run_id per row: the 200 rows APPEND, so a shared id would let row N
    // poison row N+1's projection.
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state_with_dir(tmp.path()).await;

    // (body, status, content-type, body, does it leave events behind?)
    let matrix: [(&str, StatusCode, &str, &str, bool); 14] = [
        (
            r#"{"kind":"mark_node_done","node_id":"n1"}"#,
            StatusCode::OK,
            JSON_CT,
            r#"{"ok":true}"#,
            true,
        ),
        (
            r#"{"kind":"extend_cycle","node_id":"n1","additional_iter":1}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"bump_region","region_id":"r1","additional_iter":1}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"end_region","region_id":"r1"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"pause_run"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"resume_run"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"kill_node","node_id":"n1"}"#,
            StatusCode::OK,
            JSON_CT,
            r#"{"ok":true}"#,
            true,
        ),
        // 404, and since #489 WITHOUT A TRACE. Until then this arm killed
        // the (absent) tmux session and appended its `CommandIssued` BEFORE
        // re-projecting, so the "run not found" it answered arrived after two
        // side effects. The `false` in the last column IS the fix: ADR-0037 §3
        // extends ADR-0025's "validate before writing" to the KILL, so every
        // knowable refusal — this one included — is raised ahead of both.
        // (`project` still returns `None` here because the log carries no
        // `RunStarted`, #328 / `event_log.rs`, not because it is empty.)
        // Body is JSON since #491/#601.
        (
            r#"{"kind":"restart_node","node_id":"n1"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"start_node","node_id":"n1"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            true,
        ),
        (
            r#"{"kind":"inject_artifact","path":"a/b.md","content":"x"}"#,
            StatusCode::OK,
            JSON_CT,
            r#"{"ok":true}"#,
            true,
        ),
        (
            r#"{"kind":"rename_run","name":"nn"}"#,
            StatusCode::OK,
            JSON_CT,
            r#"{"ok":true}"#,
            true,
        ),
        (
            r#"{"kind":"cleanup_run"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"retry_all"}"#,
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#,
            false,
        ),
        (
            r#"{"kind":"bogus_command"}"#,
            StatusCode::BAD_REQUEST,
            JSON_CT,
            r#"{"error":"unknown command: bogus_command"}"#,
            false,
        ),
    ];

    for (i, (req_body, want_status, want_ct, want_body, want_events)) in
        matrix.iter().enumerate()
    {
        let run_id = format!("unstarted-{i}");
        let (status, ct, body) = command_triplet(&state, &run_id, req_body).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (*want_status, *want_ct, *want_body),
            "row {i} {req_body}"
        );
        let left_events = !load_events(&state.db, &run_id).await.unwrap().is_empty();
        assert_eq!(
            left_events, *want_events,
            "row {i} {req_body}: side-effect trace changed"
        );
    }
}

#[tokio::test]
async fn every_kind_is_410_on_a_forgotten_run() {
    // ADR-0024 / #328: the tombstone gate runs BEFORE the kind match, so a
    // forgotten run answers 410 to everything — including a kind nobody
    // knows and a body missing its required fields. Move the parse ahead of
    // the gate and those two become 400. One kind was covered before this.
    let state = test_state().await;
    let run_id = "forgotten-all-kinds";
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

    for req_body in [
        r#"{"kind":"extend_cycle","node_id":"n1","additional_iter":1}"#,
        r#"{"kind":"cleanup_run"}"#,
        r#"{"kind":"inject_artifact","path":"a/b.md","content":"x"}"#,
        r#"{"kind":"rename_run","name":"nn"}"#,
        // No `node_id` — the 400 for a missing field must NOT outrank 410.
        r#"{"kind":"kill_node"}"#,
        // Unknown kind — likewise.
        r#"{"kind":"bogus_command"}"#,
        // The tombstone must outrank every NEW projection this arm
        // gained too — the `409`s, the `400 node_not_found` and the `500`.
        r#"{"kind":"restart_node","node_id":"n1","iter":1}"#,
    ] {
        let (status, ct, body) = command_triplet(&state, run_id, req_body).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (
                StatusCode::GONE,
                JSON_CT,
                format!(r#"{{"error":"run {run_id} has been forgotten"}}"#).as_str()
            ),
            "{req_body}"
        );
    }

    let events = load_events(&state.db, run_id).await.unwrap();
    assert!(events.is_empty(), "log must stay empty: {events:?}");
}

#[tokio::test]
async fn cleanup_run_error_bodies_are_json() {
    // `cleanup_run`'s error bodies are JSON like every sibling on
    // this surface — the front's `resp.json().catch()` reads them instead of
    // swallowing text/plain. The 200/409/404 status split is unchanged (it is
    // an operator contract the disk-janitor recipe reads as "reclaimed /
    // already archived / gone"); only the wire shape of the two error bodies
    // moved from text to JSON.
    let state = test_state().await;
    let run_id = "cleanup-bodies";
    seed_completed_run(&state, run_id).await;

    let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"cleanup_run"}"#).await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (
            StatusCode::OK,
            JSON_CT,
            r#"{"archived_children":[],"status":"archived"}"#
        )
    );

    let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"cleanup_run"}"#).await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (
            StatusCode::CONFLICT,
            JSON_CT,
            r#"{"error":"run is already archived"}"#
        )
    );

    let (status, ct, body) =
        command_triplet(&state, "no-such-run", r#"{"kind":"cleanup_run"}"#).await;
    assert_eq!(
        (status, ct.as_str(), body.as_str()),
        (
            StatusCode::NOT_FOUND,
            JSON_CT,
            r#"{"error":"run not found"}"#
        )
    );
}

// The truthful-response matrix (ADR-0025), without a daemon: 400 unknown target,
// 409 wrong mechanism, 200 noop, 200 spawned — all reachable from the in-crate
// router. Case D does attempt a real `tmux` call, but its failure is swallowed, so
// the verdict does not depend on tmux SUCCEEDING.
//
// The pipeline is written RUN-SCOPED on purpose: `resolve_run_pipeline_path`
// prefers it, so these tests can never silently pick up a real pipeline out of the
// developer's `$HOME/.pdo/pipelines`, which has no daemon override.
