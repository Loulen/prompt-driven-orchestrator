//! Creating a Run (`POST /runs`, JSON and multipart), its twin (`retry_all`) and
//! the edit of its run-scoped pipeline — moved from the legacy crate with the
//! code they test (#925).

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
async fn sync_run_pipeline_writes_canonical_prompt_path() {
    // Run-scoped sync must write each prompt to the canonical
    // `<stem>.prompts/<id>.md` (where every reader looks) — never the flat
    // `prompts/<id>.md` dir that no reader consults.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_test_pipeline(dir, "auto-issue");
    let state = test_state_with_dir(dir).await;

    let mut prompts = HashMap::new();
    prompts.insert("worker".to_string(), "the role prompt".to_string());

    let yaml =
        std::fs::read_to_string(dir.join(".pdo").join("pipelines").join("auto-issue.yaml"))
            .unwrap();
    sync_run_pipeline_to_template(&state, "run-xyz", "auto-issue", &yaml, &prompts);

    let pipelines = dir.join(".pdo").join("pipelines");
    let canonical = pipelines.join("auto-issue.prompts").join("worker.md");
    let flat = pipelines.join("prompts").join("worker.md");

    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        "the role prompt",
        "prompt must be written to the canonical <stem>.prompts/ dir"
    );
    assert!(
        !flat.exists(),
        "the flat prompts/ dir must never be written"
    );
}


#[test]
fn sanitize_image_filename_accepts_valid_extensions() {
    assert_eq!(
        sanitize_image_filename("photo.png"),
        Some("photo.png".into())
    );
    assert_eq!(sanitize_image_filename("img.JPG"), Some("img.JPG".into()));
    assert_eq!(sanitize_image_filename("pic.jpeg"), Some("pic.jpeg".into()));
    assert_eq!(sanitize_image_filename("anim.gif"), Some("anim.gif".into()));
    assert_eq!(
        sanitize_image_filename("modern.webp"),
        Some("modern.webp".into())
    );
    assert_eq!(sanitize_image_filename("icon.svg"), Some("icon.svg".into()));
    assert_eq!(sanitize_image_filename("old.bmp"), Some("old.bmp".into()));
}

#[test]
fn sanitize_image_filename_rejects_non_image() {
    assert_eq!(sanitize_image_filename("script.js"), None);
    assert_eq!(sanitize_image_filename("data.json"), None);
    assert_eq!(sanitize_image_filename("readme.md"), None);
    assert_eq!(sanitize_image_filename("noext"), None);
}

#[test]
fn sanitize_image_filename_strips_path_components() {
    assert_eq!(
        sanitize_image_filename("../../etc/passwd.png"),
        Some("passwd.png".into())
    );
    assert_eq!(
        sanitize_image_filename("/tmp/photo.jpg"),
        Some("photo.jpg".into())
    );
}

#[test]
fn sanitize_attachment_filename_accepts_any_extension_and_strips_paths() {
    assert_eq!(
        sanitize_attachment_filename("SPEC-779.md"),
        Ok("SPEC-779.md".into())
    );
    assert_eq!(
        sanitize_attachment_filename("../../etc/fixtures.json"),
        Ok("fixtures.json".into())
    );
    assert_eq!(
        sanitize_attachment_filename("/tmp/dump.sql"),
        Ok("dump.sql".into())
    );
    assert_eq!(sanitize_attachment_filename("noext"), Ok("noext".into()));
}

#[test]
fn sanitize_attachment_filename_refuses_empty_dots_and_the_prompt_name() {
    assert!(sanitize_attachment_filename("").is_err());
    assert!(sanitize_attachment_filename("..").is_err());
    assert!(sanitize_attachment_filename("/").is_err());
    let err = sanitize_attachment_filename("output.md").unwrap_err();
    assert!(err.contains("reserved"), "{err}");
}

#[test]
fn resolve_max_attachments_mb_stored_beats_default() {
    assert_eq!(resolve_max_attachments_mb(Some(120)), 120);
    // A stored value below 1 cannot be persisted, but the resolver still
    // treats it as unset rather than a 0-byte budget.
    assert!(resolve_max_attachments_mb(Some(0)) >= 1);
}

#[test]
fn image_files_stored_in_input_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let input_dir = tmp.path().join("_input");
    std::fs::create_dir_all(&input_dir).unwrap();
    std::fs::write(input_dir.join("output.md"), "hello").unwrap();

    let images = vec![
        InputFile {
            filename: "screenshot.png".into(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
            is_image: true,
        },
        InputFile {
            filename: "diagram.jpg".into(),
            data: vec![0xFF, 0xD8, 0xFF],
            is_image: true,
        },
    ];
    for image in &images {
        std::fs::write(input_dir.join(&image.filename), &image.data).unwrap();
    }

    assert!(input_dir.join("output.md").exists());
    assert!(input_dir.join("screenshot.png").exists());
    assert!(input_dir.join("diagram.jpg").exists());
    assert_eq!(
        std::fs::read(input_dir.join("screenshot.png")).unwrap(),
        vec![0x89, 0x50, 0x4E, 0x47]
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn retry_all_archives_and_creates_new_run() {
    // retry_all reconstructs a fresh run by re-resolving the original
    // pipeline by name from disk and creating a new git worktree for it,
    // so the repo root must be a real git repo with the template present.
    // Isolate HOME so user-scoped resolution/cleanup can't leak.
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    write_test_pipeline(tmp.path(), "retry-pipe");
    let state = test_state_with_dir(tmp.path()).await;

    let run_event = event_log::Event {
        id: None,
        run_id: "retry-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "retry-pipe",
            "input": "do the thing",
            "edges": [],
            "node_defs": [],
        })),
    };
    append_event(&state, &run_event).await.unwrap();

    let fail_event = event_log::Event {
        id: None,
        run_id: "retry-test".into(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunFailed,
        node_id: None,
        iter: None,
        payload: None,
    };
    append_event(&state, &fail_event).await.unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/retry-test/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "retry_all" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // retry_all returns create_run_core's response verbatim, which is 201 Created.
    assert_eq!(status, StatusCode::CREATED, "retry_all body: {json}");
    assert!(json.get("run_id").is_some(), "should return new run_id");

    let old_events = load_events(&state.db, "retry-test").await.unwrap();
    let old_state = event_log::project(&old_events).unwrap();
    assert_eq!(old_state.status, event_log::RunStatus::Archived);

    let new_run_id = json["run_id"].as_str().unwrap();
    let new_events = load_events(&state.db, new_run_id).await.unwrap();
    let new_state = event_log::project(&new_events).unwrap();
    assert_eq!(new_state.status, event_log::RunStatus::Running);
    assert_eq!(new_state.pipeline_name, "retry-pipe");
    assert_eq!(new_state.input.as_deref(), Some("do the thing"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn retry_all_of_a_legacy_null_target_run_lands_on_the_resolved_repo() {
    // `retry_all` archives the original BEFORE creating the replacement.
    // A run recorded before the create boundary was hardened carries
    // `target_repo: null`; forwarding that raw would 400 at the now-mandatory
    // chokepoint — after the archive — i.e. archived with no replacement, work
    // unreachable. So the retry reads the RESOLVED repo and pins it explicitly.
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    write_test_pipeline(tmp.path(), "retry-pipe");
    let state = test_state_with_dir(tmp.path()).await;

    for (kind, payload) in [
        (
            event_log::EventKind::RunStarted,
            Some(serde_json::json!({
                "pipeline_name": "retry-pipe",
                "input": "legacy work",
                "edges": [],
                "node_defs": [],
                // NOTE: no `target_repo` — the whole point.
            })),
        ),
        (event_log::EventKind::RunFailed, None),
    ] {
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: "legacy-null-target".into(),
                ts: pdo_kernel::clock::now_iso(),
                kind,
                node_id: None,
                iter: None,
                payload,
            },
        )
        .await
        .unwrap();
    }

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/legacy-null-target/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "retry_all" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the retry must not 400 after archiving; body: {json}"
    );

    let new_run_id = json["run_id"].as_str().unwrap();
    let new_state = event_log::project(&load_events(&state.db, new_run_id).await.unwrap())
        .expect("the replacement run must exist");
    assert_eq!(
        new_state.target_repo.as_deref(),
        state.repo_root.to_str(),
        "the retry lands where the original actually ran, now recorded explicitly"
    );
}

#[tokio::test]
async fn retry_all_rejected_on_running_state() {
    let state = test_state().await;

    let run_event = event_log::Event {
        id: None,
        run_id: "retry-running".into(),
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
                .uri("/runs/retry-running/commands")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "retry_all" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[test]
fn prompt_required_pipeline_rejects_empty_input() {
    assert!(validate_run_input(true, "").is_err());
    assert!(validate_run_input(true, "   \n\t ").is_err());
}

#[test]
fn prompt_required_pipeline_accepts_non_empty_input() {
    assert!(validate_run_input(true, "fix the auth bug").is_ok());
}

#[test]
fn prompt_optional_pipeline_accepts_empty_input() {
    assert!(validate_run_input(false, "").is_ok());
    assert!(validate_run_input(false, "   ").is_ok());
}

#[test]
fn prompt_optional_pipeline_still_accepts_a_prompt() {
    assert!(validate_run_input(false, "extra context").is_ok());
}

#[test]
fn placeholder_name_is_deterministic_from_run_id_timestamp() {
    assert_eq!(
        placeholder_run_name("20260629-150013-4a2f1c0"),
        "Untitled run 20260629-150013"
    );
}

#[test]
fn run_name_hint_matrix() {
    use prompt_augmenter::RunNameHint;
    // The hint is now gated on the RESOLVED `auto_name`, not on the name.
    // With auto-naming OFF the manager is never told to rename — `UserProvided`
    // regardless of name or input (the display name is chosen by the caller).
    assert_eq!(
        run_name_hint(false, Some("My Run"), ""),
        RunNameHint::UserProvided
    );
    assert_eq!(
        run_name_hint(false, Some("My Run"), "some input"),
        RunNameHint::UserProvided
    );
    assert_eq!(run_name_hint(false, None, ""), RunNameHint::UserProvided);
    assert_eq!(
        run_name_hint(false, None, "do a thing"),
        RunNameHint::UserProvided
    );

    // With auto-naming ON the pre-#338 behaviour holds, gated on input only.
    // No (meaningful) input → deterministic placeholder, renamed best-effort later.
    assert_eq!(run_name_hint(true, None, ""), RunNameHint::Placeholder);
    assert_eq!(
        run_name_hint(true, None, "   \n\t "),
        RunNameHint::Placeholder
    );
    assert_eq!(run_name_hint(true, Some(""), ""), RunNameHint::Placeholder);
    // Real input → manager derives the name from `_input` immediately.
    assert_eq!(
        run_name_hint(true, None, "do a thing"),
        RunNameHint::DeriveFromInput
    );
    assert_eq!(
        run_name_hint(true, Some("ignored when auto"), "do a thing"),
        RunNameHint::DeriveFromInput
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_refuses_pipeline_with_dangling_port() {
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();

    // `worker` declares output `result`, but the edge sources `resullt`:
    // a dangling port reference that would stall the run mid-flight.
    let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir).unwrap();
    let yaml = format!(
        "name: dangling-pipe\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    outputs:\n      - name: result\nedges:\n  - source: {{ node: worker, port: resullt }}\n    target: {{ node: end, port: result }}\n"
    );
    std::fs::write(pipelines_dir.join("dangling-pipe.yaml"), yaml).unwrap();
    // The target repo is now checked FIRST in `create_run_inner`, so this
    // test must name a valid one — otherwise it would 400 for the wrong reason
    // and silently stop testing #211.
    init_test_repo(tmp.path());

    let state = test_state_with_dir(tmp.path()).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"pipeline": "dangling-pipe", "input": "do the thing", "target_repo": "{}"}}"#,
                    tmp.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a dangling port reference must refuse the launch"
    );
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains("'resullt'") && error.contains("worker"),
        "error must name the missing port and the edge; got: {error}"
    );
    assert!(
        error.contains("end"),
        "error must identify the edge (both endpoints); got: {error}"
    );

    // No run must have been created.
    let app = build_router(state);
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
    assert!(runs.is_empty(), "no run must be created on refusal");
}

/// Assert that a `POST /runs` body was refused *for the target-repo reason*, and
/// that the refusal happened before ANY effect: no Run, and — the point of the
/// 2026-07-29 incident — no `.pdo/runs/` tree under the daemon's own root.
async fn assert_create_run_refused_for_target_repo(state: &Arc<TestState>, body: &str) {
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a Run without a named target repo must be refused; body was: {body}"
    );
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("target_repo"),
        "the error must name the field; got: {error}"
    );
    assert!(
        error.contains("absolute path"),
        "the error must be actionable — say WHAT to pass; got: {error}"
    );
    assert_no_run_and_no_worktree(state).await;
}

/// The AC1 pair: zero Runs listed, and zero worktree scaffolding under the
/// daemon's storage root.
async fn assert_no_run_and_no_worktree(state: &Arc<TestState>) {
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
    assert!(runs.is_empty(), "no run must be created on refusal");
    // The `.pdo/runs` dir may pre-exist (the pipeline watcher creates it to
    // watch run-scoped edits), so assert on its CONTENTS: nothing was staged.
    let runs_dir = state.repo_root.join(".pdo").join("runs");
    let entries: Vec<_> = std::fs::read_dir(&runs_dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        entries.is_empty(),
        "no worktree scaffolding may appear under the daemon's own root: {entries:?}"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_without_target_repo_is_400() {
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let state = test_state_with_dir(tmp.path()).await;

    // Everything else about this body is valid — only the target repo is
    // missing. Before #470 this created a Run in the daemon's own repo.
    assert_create_run_refused_for_target_repo(
        &state,
        r#"{"pipeline": "some-pipe", "input": "do the thing"}"#,
    )
    .await;
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_with_unknown_field_is_400_naming_it() {
    // An unknown top-level field is a named `400`, never silently dropped
    // (the symptom the issue cites is a `target_repos` typo thrown away, launching
    // the run mono-repo). The check runs before any effect: no Run is created.
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let repo = tmp.path().to_string_lossy().into_owned();
    let state = test_state_with_dir(tmp.path()).await;

    let app = build_router(state.clone());
    let body = format!(
        r#"{{"pipeline":"some-pipe","input":"x","target_repo":"{repo}","target_repoz":[]}}"#
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("target_repoz"),
        "the error must name the unknown field; got: {error}"
    );
    assert_no_run_and_no_worktree(&state).await;
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_with_unknown_field_multipart_is_400() {
    // The symmetric fix on the multipart (browser) path: an unrecognised form
    // field is named, not silently ignored by the old `_ => {}` arm.
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let state = test_state_with_dir(tmp.path()).await;

    const BOUNDARY: &str = "----pdoTestBoundary601";
    let mut body = String::new();
    for (name, value) in [("pipeline", "some-pipe"), ("input", "x"), ("bogus", "v")] {
        body.push_str(&format!("--{BOUNDARY}\r\n"));
        body.push_str(&format!(
            "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{BOUNDARY}--\r\n"));

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        json["error"].as_str().unwrap_or_default().contains("bogus"),
        "the multipart error must name the unknown field; got: {json}"
    );
    assert_no_run_and_no_worktree(&state).await;
}

#[test]
fn create_run_known_fields_cover_the_struct() {
    // Guards the allow-list against drift: a payload carrying EXACTLY the
    // accepted keys must pass the unknown-field gate AND deserialize into the
    // typed request. A field added to `CreateRunRequest` but not to
    // `CREATE_RUN_FIELDS` would be rejected by the gate at runtime; one added
    // to the list but not the struct would be a dead entry. Values are
    // type-appropriate (`#[serde(default)]` only fills an ABSENT key, so a
    // present `null` on a `Vec`/`HashMap` field would fail — hence `{}`/`[]`).
    let value = serde_json::json!({
        "pipeline": "p",
        "input": "i",
        "variables": {},
        "pipeline_id": null,
        "target_repo": null,
        "target_repos": [],
        "source_branch": null,
        "name": null,
        "triggered_by": null,
        "sandbox": null,
        "harness": null,
        "agent_choice": null,
        "skills": [],
        "auto_name": null,
        "auto_fail": null,
        "provisioning": {},
    });
    // The payload's keys are exactly the allow-list (pins the lock-step).
    let keys: std::collections::BTreeSet<String> =
        value.as_object().unwrap().keys().cloned().collect();
    let allowed: std::collections::BTreeSet<String> =
        CREATE_RUN_FIELDS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        keys, allowed,
        "the test payload must list exactly CREATE_RUN_FIELDS"
    );

    assert!(
        reject_unknown_create_run_fields(&value).is_ok(),
        "every listed field must be accepted by the gate"
    );
    let parsed: Result<CreateRunRequest, _> = serde_json::from_value(value);
    assert!(
        parsed.is_ok(),
        "the accepted field set must deserialize into CreateRunRequest: {:?}",
        parsed.err()
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_with_blank_target_repo_is_400() {
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let state = test_state_with_dir(tmp.path()).await;

    for blank in ["", "   "] {
        assert_create_run_refused_for_target_repo(
            &state,
            &format!(
                r#"{{"pipeline": "some-pipe", "input": "do the thing", "target_repo": "{blank}"}}"#
            ),
        )
        .await;
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_with_blank_target_repo_multipart_is_400() {
    // Distinct case: the multipart path NORMALISES an empty field to `None`
    // (`if !v.is_empty()`), so a blank there is not the same input shape the
    // JSON path sees. Both must land on the same refusal.
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let state = test_state_with_dir(tmp.path()).await;

    const BOUNDARY: &str = "----pdoTestBoundary470";
    let mut body = String::new();
    for (name, value) in [
        ("pipeline", "some-pipe"),
        ("input", "do the thing"),
        ("target_repo", "   "),
    ] {
        body.push_str(&format!("--{BOUNDARY}\r\n"));
        body.push_str(&format!(
            "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{BOUNDARY}--\r\n"));

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a blank multipart target_repo must be refused"
    );
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("target_repo"),
        "the error must name the field; got: {error}"
    );
    assert_no_run_and_no_worktree(&state).await;
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_run_with_non_git_target_repo_is_400() {
    // Non-regression on `validate_target_repo`: naming a directory is not
    // enough, it must be a git repository (AC3).
    let _home = FakeHome::new();
    let tmp = tempfile::tempdir().unwrap();
    write_test_pipeline(tmp.path(), "some-pipe");
    init_test_repo(tmp.path());
    let state = test_state_with_dir(tmp.path()).await;

    let not_a_repo = tempfile::tempdir().unwrap();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"pipeline": "some-pipe", "input": "x", "target_repo": "{}"}}"#,
                    not_a_repo.path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let error = json["error"].as_str().unwrap_or_default();
    assert!(error.contains("not a git repository"), "got: {error}");
    assert_no_run_and_no_worktree(&state).await;
}
