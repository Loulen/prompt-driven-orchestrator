//! What the Run's surfaces read: its detail's cost and LOC, its artifacts, its
//! refs and a file at one of them, its source drift, its repository — moved from
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
async fn get_nonexistent_run_returns_404() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_payload_carries_sessions_spawned_and_cost() {
    // GET /runs/:id exposes `sessions_spawned` (a number). Both
    // `loc` and `cost` are derived-on-read and omitted here — the seeded run
    // has no `pdo/run-<id>` branch and no `~/.claude` transcript dir, so each
    // renders "—" in the UI (absent, not `Some(0)`). The positive-path cost
    // aggregation is covered by `run_cost::tests::compute_run_cost_*`.
    let state = test_state().await;
    let run_id = "stats-payload";
    seed_completed_run(&state, run_id).await; // exactly one NodeStarted

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

    assert_eq!(body["sessions_spawned"], 1);
    // No run branch for this seeded run → `loc` is omitted (None), not Some(0).
    assert!(body.get("loc").is_none() || body["loc"].is_null());
    // No transcript dir for this seeded run → `cost` is omitted (None) → "—".
    assert!(body.get("cost").is_none() || body["cost"].is_null());
}

#[tokio::test]
async fn get_run_payload_carries_has_manager_as_an_observed_fact() {
    // Manager on demand: the wire's `has_manager` is an OBSERVED tmux fact,
    // probed at fetch time — never a projected flag. In the test env no tmux
    // session exists, so a freshly seeded run reads `false` even though no
    // event ever said otherwise; a spawned-then-swept session would read the
    // same way, which is exactly the desynchronisation a projected flag
    // cannot survive.
    let state = test_state().await;
    let run_id = "manager-payload";
    seed_completed_run(&state, run_id).await;

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
    assert_eq!(body["has_manager"], false);
}

#[tokio::test]
async fn start_manager_on_a_run_without_a_worktree_is_refused() {
    // Manager on demand: the start endpoint is always available (the
    // `manager_enabled` setting gates only the automatic spawn), but a run
    // whose worktree is gone (archived/cleaned) has nothing for a manager
    // to work in — refused with a 409 naming why, never a spawn into a dead
    // cwd.
    let state = test_state().await;
    let run_id = "manager-start-noworktree";
    seed_completed_run(&state, run_id).await;

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/manager/start"))
                .body(Body::empty())
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
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("worktree no longer exists"),
        "got {body}"
    );
}

#[tokio::test]
async fn start_manager_on_an_unknown_run_is_a_404() {
    let state = test_state().await;
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/no-such-run/manager/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stop_manager_on_a_dead_session_is_a_calm_noop() {
    // Manager on demand: stopping a session that is already gone answers
    // 200 with `stopped: false` — and still writes its ManagerStopped
    // event, which the projection ignores.
    let state = test_state().await;
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs/manager-stop-ghost/manager/stop")
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
    assert_eq!(body["ok"], true);
    assert_eq!(body["stopped"], false);
}

#[test]
fn run_detail_projects_the_same_cost_onto_its_parent_node() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let projects = home.path().join(".claude/projects");
    let run_id = "node-cost";
    let working_dir = worktrees::worktree_dir_for_run(repo.path(), run_id);
    let project = projects.join(pdo_harness::claude_transcript::cc_project_dirname(
        &working_dir,
    ));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("sid.jsonl"),
        "{\"type\":\"assistant\",\"requestId\":\"r\",\"message\":{\"id\":\"m\",\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":1000000}}}\n",
    )
    .unwrap();
    let events = vec![
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        },
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "claude",
                "session_id": "sid"
            })),
        },
    ];
    let mut run_state = event_log::project(&events).unwrap();

    augment_run_state_from_disk(
        &mut run_state,
        &events,
        &projects,
        &pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path()),
        repo.path(),
        &price_table::PriceTable::load(home.path()),
    );

    let node_cost = run_state.nodes["worker"].cost.as_ref().unwrap();
    assert_eq!(node_cost.form, Some(event_log::CostForm::Derived));
    assert!((node_cost.usd.unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(node_cost.executions, 1);
    assert_eq!(run_state.cost.as_ref().unwrap().usd, node_cost.usd.unwrap());
}

#[test]
fn parse_numstat_sums_text_and_counts_binary_without_lines() {
    // Text rows sum +/- ; binary rows show `-` so contribute 0 lines
    // but still count as a changed file. A trailing blank line is ignored.
    let out = "3\t1\tsrc/a.rs\n2\t1\tsrc/b.rs\n-\t-\tassets/logo.png\n\n";
    let loc = parse_numstat(out);
    assert_eq!(
        loc,
        event_log::LocStat {
            insertions: 5,
            deletions: 2,
            files_changed: 3,
        }
    );
}

#[test]
fn parse_numstat_empty_is_zero() {
    assert_eq!(
        parse_numstat(""),
        event_log::LocStat {
            insertions: 0,
            deletions: 0,
            files_changed: 0,
        }
    );
}

#[test]
fn compute_run_loc_three_dot_excludes_pdo_and_drops_when_branch_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };

    // Base file on HEAD so the run branch can delete a line from it.
    std::fs::write(repo.join("code.txt"), "keep1\nremove\nkeep2\n").unwrap();
    git(&["add", "code.txt"]);
    git(&["commit", "-m", "add code"]);

    let default_branch =
        String::from_utf8_lossy(&git(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
            .trim()
            .to_string();
    let run_id = "loc-test";
    let branch = format!("pdo/run-{run_id}");

    // Run branch: a +3/-1 source diff PLUS a tracked `.pdo/` change that must
    // be excluded (so files_changed stays 1, not 2).
    git(&["checkout", "-b", &branch]);
    std::fs::write(repo.join("code.txt"), "keep1\nadd1\nadd2\nadd3\nkeep2\n").unwrap();
    std::fs::create_dir_all(repo.join(".pdo")).unwrap();
    std::fs::write(repo.join(".pdo/noise.txt"), "a\nb\nc\nd\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "run changes"]);
    git(&["checkout", &default_branch]);

    // Tier-3 legacy path: no fork_sha/source_branch recorded, so the base is "HEAD".
    // This test never parks HEAD off the fork line, so the behaviour is unchanged.
    let loc = compute_run_loc(repo, run_id, "HEAD").expect("branch present -> Some");
    assert_eq!(
        loc,
        event_log::LocStat {
            insertions: 3,
            deletions: 1,
            files_changed: 1,
        },
        ".pdo/ excluded so only code.txt counts"
    );

    // Three-dot base = fork point: advancing HEAD after the fork must NOT
    // change the run's LOC (two-dot would report the new commit as deletions).
    std::fs::write(repo.join("other.txt"), "x\ny\n").unwrap();
    git(&["add", "other.txt"]);
    git(&["commit", "-m", "advance main"]);
    let loc_after = compute_run_loc(repo, run_id, "HEAD").expect("still Some");
    assert_eq!(
        loc_after, loc,
        "three-dot base stays at the fork point as HEAD advances"
    );

    // Branch gone (cleanup) -> None -> UI renders "—" (not "0").
    git(&["branch", "-D", &branch]);
    assert!(
        compute_run_loc(repo, run_id, "HEAD").is_none(),
        "missing run branch -> None"
    );
}

#[test]
fn compute_run_loc_clean_diff_is_some_zero_not_none() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let run_id = "loc-clean";
    // Branch at HEAD with no divergence: a successful but EMPTY diff must be
    // Some({0,0,0}) (UI "0"), distinct from None (UI "—").
    let out = std::process::Command::new("git")
        .args(["branch", &format!("pdo/run-{run_id}")])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        compute_run_loc(repo, run_id, "HEAD").expect("branch present -> Some"),
        event_log::LocStat {
            insertions: 0,
            deletions: 0,
            files_changed: 0,
        }
    );
}

#[test]
fn compute_run_loc_ignores_parked_checkout_and_uses_run_source_branch() {
    // #417 regression: the shared checkout is parked on a divergent branch that
    // forks BEFORE the run's fork point. The pre-fix `HEAD...` range collapsed the
    // merge-base onto the common ancestor and swept in every commit the fork branch
    // gained since (the phantom). Basing the diff on the Run's own fork point
    // (`default_branch`, standing in for the frozen `fork_sha`) must report ONLY the
    // run's own 1-line change, regardless of where HEAD is parked.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo); // c0 = "initial" (README.md) on the default branch
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };

    // `init_test_repo` is a bare `git init` → recover the default branch name.
    let default_branch =
        String::from_utf8_lossy(&git(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
            .trim()
            .to_string();

    // A divergent branch off c0 with its own commit — the head the operator parks on.
    git(&["checkout", "-b", "integration/prd-403-sandbox"]);
    std::fs::write(repo.join("sandbox.txt"), "sandbox\n").unwrap();
    git(&["add", "sandbox.txt"]);
    git(&["commit", "-m", "parked: unrelated work"]);

    // Advance the default branch two commits PAST the fork — this is the phantom
    // payload the buggy merge-base would sweep in.
    git(&["checkout", &default_branch]);
    std::fs::write(repo.join("file_a.txt"), "a1\na2\na3\n").unwrap();
    git(&["add", "file_a.txt"]);
    git(&["commit", "-m", "main: +3 in file_a"]);
    std::fs::write(repo.join("file_b.txt"), "b1\nb2\n").unwrap();
    git(&["add", "file_b.txt"]);
    git(&["commit", "-m", "main: +2 in file_b"]); // <- the run's fork point

    // Cut the run branch from the default-branch tip and add ONE real line.
    let run_id = "loc-parked-fork";
    let branch = format!("pdo/run-{run_id}");
    git(&["checkout", "-b", &branch]);
    std::fs::write(repo.join("run_only.txt"), "the only real change\n").unwrap();
    git(&["add", "run_only.txt"]);
    git(&["commit", "-m", "run change"]);

    // PARK the checkout on the divergent branch (the exact #417 / #451 condition).
    git(&["checkout", "integration/prd-403-sandbox"]);

    // Based on the run's fork point, ONLY run_only.txt counts — never file_a/file_b.
    // Under the pre-fix `HEAD...` the parked checkout would inflate this to {6,0,3}.
    assert_eq!(
        compute_run_loc(repo, run_id, &default_branch).expect("branch present -> Some"),
        event_log::LocStat {
            insertions: 1,
            deletions: 0,
            files_changed: 1,
        },
        "parked divergent HEAD must not displace the fork-point base"
    );

    // Doc-only variant: a run cut at the fork tip with NO commit changed nothing.
    // With HEAD still parked on the divergent branch, the honest answer is {0,0,0}
    // (the triage's "even non-isolated, zero files" case), not the phantom.
    let doc_run = "loc-parked-non-isolated";
    git(&["branch", &format!("pdo/run-{doc_run}"), &default_branch]);
    assert_eq!(
        compute_run_loc(repo, doc_run, &default_branch).expect("branch present -> Some"),
        event_log::LocStat {
            insertions: 0,
            deletions: 0,
            files_changed: 0,
        },
        "a run that changed nothing reads 0, not the parked-checkout phantom"
    );
}

#[tokio::test]
async fn artifact_returns_404_for_missing_run() {
    let state = test_state().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/nope/artifact?path=planner/iter-1/plan.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn artifact_returns_content_with_text_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-content-test";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
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
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("# Plan"));
}

// #333 / ADR-0028 security invariant: an `output.html` artifact is served as
// `text/markdown`, NEVER `text/html`. Serving it as a top-level HTML
// document at the daemon origin (no auth, no CSP) would execute
// agent-authored (prompt-injectable) script. The html artifact is only ever
// rendered client-side inside a sandboxed iframe. This test fails loudly if
// a future contributor "helpfully" maps `.html → text/html`.
#[tokio::test]
async fn artifact_serves_html_as_text_markdown_never_text_html() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-html-mime";
    seed_io_test(tmp.path(), run_id);
    // Seed a real html artifact under a port dir (canonicalize needs it).
    let html_body = "<!doctype html><html><body><h1>Report</h1>\
         <script>window.__pdo_pwned=1</script></body></html>";
    std::fs::write(
        tmp.path()
            .join(".pdo/runs")
            .join(run_id)
            .join("worktree/.pdo/artifacts/planner/iter-1/plan/output.html"),
        html_body,
    )
    .unwrap();
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/artifact?path=planner/iter-1/plan/output.html"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap();
    assert_eq!(ct, "text/markdown");
    assert_ne!(ct, "text/html");
    // The body is returned verbatim (the daemon does not sanitize) — the
    // safety comes entirely from the non-executable content type + the
    // client-side sandboxed iframe.
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert_eq!(text, html_body);
}

#[tokio::test]
async fn artifact_returns_400_on_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-traversal";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
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
    // Should be 400 (traversal) or 404 (file not found after canonicalize fails)
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
        "expected 400 or 404 for traversal, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn artifact_returns_404_for_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-missing-file";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/artifact?path=planner/iter-1/nonexistent.md"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// A `files` port folder of the seeded io run: a PDF and a CSV.
fn seed_port_files(dir: &Path, run_id: &str) -> PathBuf {
    let port = dir
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts/planner/iter-1/exports");
    std::fs::create_dir_all(port.join("nested")).unwrap();
    std::fs::write(port.join("report.pdf"), b"%PDF-1.4\n\xff\xfe binary").unwrap();
    std::fs::write(port.join("rows.csv"), "a,b\n1,2\n").unwrap();
    port
}

async fn get(state: Arc<TestState>, uri: String) -> Response {
    build_router(state)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_downloaded_artifact_is_an_attachment_with_a_readable_name() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-download";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let resp = get(
        state,
        format!("/runs/{run_id}/artifact?path=planner/iter-1/plan/output.md&download=true"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"planner-plan.md\"; filename*=UTF-8''planner-plan.md"
    );
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"# Plan\nDo stuff.");
}

#[tokio::test]
async fn a_binary_file_of_a_port_downloads_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-download-binary";
    seed_io_test(tmp.path(), run_id);
    seed_port_files(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let resp = get(
        state,
        format!("/runs/{run_id}/artifact?path=planner/iter-1/exports/report.pdf&download=true"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("attachment; filename=\"report.pdf\""));
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"%PDF-1.4\n\xff\xfe binary");
}

#[tokio::test]
async fn a_port_folder_downloads_as_a_zip_of_its_files() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-zip";
    seed_io_test(tmp.path(), run_id);
    seed_port_files(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let resp = get(
        state,
        format!("/runs/{run_id}/artifact/zip?path=planner/iter-1/exports"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );
    assert!(resp
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("attachment; filename=\"planner-exports.zip\""));
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).unwrap();
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["report.pdf", "rows.csv"]);
    let mut csv = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("rows.csv").unwrap(), &mut csv).unwrap();
    assert_eq!(csv, "a,b\n1,2\n");
}

#[tokio::test]
async fn a_zip_never_leaves_the_artifacts_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-zip-traversal";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    let resp = get(state, format!("/runs/{run_id}/artifact/zip?path=../../..")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_zip_of_a_file_or_a_missing_folder_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "artifact-zip-missing";
    seed_io_test(tmp.path(), run_id);
    let state = test_state_with_dir(tmp.path()).await;
    seed_io_run_events(&state, run_id).await;

    for path in ["planner/iter-1/plan/output.md", "planner/iter-1/nope"] {
        let resp = get(
            state.clone(),
            format!("/runs/{run_id}/artifact/zip?path={path}"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

// --- Transition guard wiring (#212, closes #195 #196 #197 #198 #201) ---

fn head_sha(repo: &std::path::Path) -> String {
    structured_diff::rev_parse(repo, "HEAD").expect("HEAD resolves")
}

// ── Dérive de la source (#803, ADR-0070 §4) ──────────────────────────────

/// A repo carrying a Run: a `pdo/run-<id>` branch cut from `main`, with the
/// fork SHA frozen in the projected state the way `RunStarted` does.
fn repo_with_run(
    run_id: &str,
    source: &str,
) -> (tempfile::TempDir, tempfile::TempDir, event_log::RunState) {
    let (origin, work) = init_repo_with_remote();
    let fork = head_sha(work.path());
    run_git(
        work.path(),
        &["branch", &run_refs::tip_branch(run_id), &fork],
    );
    let mut run_state = event_log::RunState::new(run_id.into(), "pipe".into());
    run_state.target_repo = Some(work.path().display().to_string());
    run_state.source_branch = Some(source.into());
    run_state.fork_sha = Some(fork);
    (origin, work, run_state)
}

/// Commit `n` times on a branch without disturbing the checkout: a worktree in
/// a temp dir, committed in, then removed.
fn commit_on_branch(repo: &std::path::Path, branch: &str, n: usize, prefix: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wt");
    run_git(
        repo,
        &["worktree", "add", "-q", path.to_str().unwrap(), branch],
    );
    commit_n(&path, n, prefix);
    run_git(
        repo,
        &["worktree", "remove", "--force", path.to_str().unwrap()],
    );
}

#[test]
fn source_drift_counts_both_sides_since_the_fork() {
    let run_id = "eaea593";
    let (origin, work, run_state) = repo_with_run(run_id, "main");
    // One commit on the Run's branch, one on the local source, three on origin.
    commit_on_branch(work.path(), &run_refs::tip_branch(run_id), 1, "run-side");
    commit_n(work.path(), 1, "source-side");
    commit_n(origin.path(), 3, "origin-side");
    run_git(work.path(), &["fetch", "--quiet", "origin"]);

    let drift = source_drift(work.path(), run_id, &run_state);

    match drift {
        SourceDrift::Available {
            ahead,
            behind,
            local_behind,
            upstream,
            upstream_behind,
            source_branch,
            ..
        } => {
            assert_eq!(source_branch, "main");
            assert_eq!(ahead, 1, "what the Run produced");
            // ONE number on the chip (spec #801 Q3): the MAX of the two sides,
            // never their sum — the local commit and the three on origin are
            // different commits off the same fork, and "4 arrived" would claim
            // a distance nobody has to travel. The split lives in the tooltip,
            // which is exactly the design's `main +1 · origin/main +3` line.
            assert_eq!(behind, 3);
            assert_eq!(local_behind, Some(1));
            assert_eq!(upstream.as_deref(), Some("origin/main"));
            assert_eq!(upstream_behind, Some(3));
        }
        other => panic!("expected an available drift, got {other:?}"),
    }
}

#[test]
fn source_drift_is_zero_for_a_run_that_just_started() {
    let run_id = "fresh01";
    let (_origin, work, run_state) = repo_with_run(run_id, "main");

    match source_drift(work.path(), run_id, &run_state) {
        SourceDrift::Available { ahead, behind, .. } => {
            assert_eq!((ahead, behind), (0, 0));
        }
        other => panic!("expected an available drift, got {other:?}"),
    }
}

#[test]
fn source_drift_is_unavailable_once_the_run_branch_is_gone() {
    let run_id = "cleaned";
    let (_origin, work, run_state) = repo_with_run(run_id, "main");
    run_git(
        work.path(),
        &["branch", "-D", &run_refs::tip_branch(run_id)],
    );

    match source_drift(work.path(), run_id, &run_state) {
        // An archived Run is the expected end of a Run's life: named, not an
        // error, and never a fabricated zero.
        SourceDrift::Unavailable { reason } => {
            assert!(reason.contains("branch"), "reason was: {reason}");
        }
        other => panic!("expected unavailable, got {other:?}"),
    }
}

#[test]
fn source_drift_of_a_remote_source_has_no_local_side() {
    let run_id = "remotesrc";
    let (_origin, work, mut run_state) = repo_with_run(run_id, "origin/feature-remote-only");
    // The fork of this Run is `main`'s tip; `origin/feature-remote-only` is one
    // commit past it (the clone's fixture), so the source side counts 1.
    run_state.fork_sha = Some(head_sha(work.path()));

    match source_drift(work.path(), run_id, &run_state) {
        SourceDrift::Available {
            behind,
            local_behind,
            upstream,
            ..
        } => {
            assert_eq!(behind, 1);
            assert!(
                local_behind.is_none(),
                "a tracking ref has no local branch to count"
            );
            assert!(upstream.is_none(), "and it IS the upstream");
        }
        other => panic!("expected an available drift, got {other:?}"),
    }
}

#[tokio::test]
async fn source_drift_endpoint_answers_200_even_when_it_cannot_measure() {
    // The contract the Run view leans on: `unavailable` is a 200 with a reason,
    // never a 4xx/5xx. An archived Run whose branch was cleaned up is the
    // expected end of a Run's life, and an error path there would make the
    // panel shout about something that is simply over.
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let run_id = "drift-run";
    let state = test_state_with_dir(daemon_dir.path()).await;
    seed_run_in_other_repo(&state, target_dir.path(), run_id).await;

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/source-drift"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    // This fixture records no source branch, so the measurement cannot be made.
    assert_eq!(json["state"], "unavailable");
    assert!(
        json["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "the reason is what the panel shows instead of numbers: {json}"
    );
}

#[tokio::test]
async fn effective_repo_root_falls_back_only_for_legacy_null_target_runs() {
    let state = test_state().await;

    let mut run_state = event_log::RunState::new("test".into(), "pipe".into());

    // #470/AC7: the fallback is READ-side ONLY. A `target_repo: null` here can
    // only come from a `run_started` recorded before the create boundary was
    // hardened — `POST /runs` now answers 400 rather than defaulting (see
    // `create_run_without_target_repo_is_400`). Resolving those old records to
    // the daemon root is what keeps the archive, the cost axes and the liveness
    // sweeps readable; do not symmetrise it away.
    let default_root = effective_repo_root(&state, &run_state);
    assert_eq!(default_root, state.repo_root);

    // With target_repo — uses it
    run_state.target_repo = Some("/custom/repo".into());
    assert_eq!(
        effective_repo_root(&state, &run_state),
        PathBuf::from("/custom/repo")
    );
}

#[test]
fn run_state_projection_includes_target_repo() {
    let events = vec![event_log::Event {
        id: None,
        run_id: "test-run".into(),
        ts: "2026-01-01T00:00:00Z".into(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "hello",
            "target_repo": "/custom/repo",
            "source_branch": "feature-branch",
            "edges": [],
            "node_defs": [],
        })),
    }];

    let state = event_log::project(&events).unwrap();
    assert_eq!(state.target_repo.as_deref(), Some("/custom/repo"));
    assert_eq!(state.source_branch.as_deref(), Some("feature-branch"));
}

#[test]
fn run_state_projection_no_target_repo_when_absent() {
    let events = vec![event_log::Event {
        id: None,
        run_id: "test-run".into(),
        ts: "2026-01-01T00:00:00Z".into(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "hello",
            "edges": [],
            "node_defs": [],
        })),
    }];

    let state = event_log::project(&events).unwrap();
    assert!(state.target_repo.is_none());
    assert!(state.source_branch.is_none());
}

#[tokio::test]
async fn file_at_ref_returns_content_defaults_to_tip_and_404s_missing() {
    // #748 AC3: the content endpoint the next ticket's context expansion uses.
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "file-at-ref";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
    std::fs::create_dir_all(wt_dir.join("src")).unwrap();
    std::fs::write(wt_dir.join("src/new file.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "add"]);

    let app = build_router(state);
    // Default ref = the Run tip.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/file?path=src%2Fnew%20file.rs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    assert_eq!(body_string(resp).await, "fn a() {}\nfn b() {}\n");

    // At the fork point the file does not exist yet → 404.
    let fork = git_in(target, &["rev-parse", "HEAD"]);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/file?path=src%2Fnew%20file.rs&ref={fork}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // README exists at the fork point.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/file?path=README.md&ref={fork}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "# test\n");

    // Parent-escaping paths are refused.
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/file?path=..%2Fetc%2Fpasswd"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn required_target_repo_rejects_absent() {
    let err = required_target_repo(None).unwrap_err();
    assert!(err.contains("target_repo"), "message was: {err}");
    // Actionable on purpose: the caller must learn what to pass.
    assert!(err.contains("absolute path"), "message was: {err}");
}

#[test]
fn required_target_repo_rejects_empty() {
    let err = required_target_repo(Some("")).unwrap_err();
    assert!(err.contains("target_repo"), "message was: {err}");
}

#[test]
fn required_target_repo_rejects_whitespace() {
    let err = required_target_repo(Some("   \t ")).unwrap_err();
    assert!(err.contains("target_repo"), "message was: {err}");
}

#[test]
fn required_target_repo_trims_and_accepts() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    let padded = format!("  {}  ", tmp.path().to_str().unwrap());
    // The trimmed value is the canonical one: what we validate is what the
    // caller gets back, so nothing downstream can diverge from it.
    assert_eq!(required_target_repo(Some(&padded)).unwrap(), tmp.path());
}

// ── #802 / ADR-0070: écart amont, dates, and the fetch verb ──────────────
//
// Every test below drives a REAL pair of repos (a clone of a filesystem
// "origin"), because the whole point of the feature is what git reports about
// an actual divergence. The prior art is `validate_source_branch`'s suite.
