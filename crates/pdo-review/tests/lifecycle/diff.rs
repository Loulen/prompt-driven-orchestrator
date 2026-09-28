//! The raw and the structured diff of a Run, over git in temporary
//! repositories (moved from the legacy crate's in-crate tests, #926).

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pdo_run::adapters::disk_live_facts::{compute_run_loc, run_diff_base};
use pdo_workspace::adapters::git::worktrees::{self, create_worktree};
use tower::ServiceExt;

use crate::harness::{
    append_event, build_router, event_log, load_events, test_state, test_state_with_dir,
};
use crate::helpers::{
    body_string, git_in, init_test_repo, seed_run_in_other_repo, seed_run_with_node,
};

#[tokio::test]
async fn run_diff_returns_404_for_nonexistent_run() {
    let state = test_state().await;
    let app = build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs/no-such-run/diff")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn run_diff_returns_empty_diff_when_no_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-empty";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
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
    assert!(body.is_empty() || body.trim().is_empty());
}

#[tokio::test]
async fn run_diff_returns_aggregate_diff_with_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-agg";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    // Make a change on the pipeline branch
    std::fs::write(wt_dir.join("new_file.rs"), "fn hello() {}\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&wt_dir)
            .output()
            .unwrap()
    };
    run(&["add", "new_file.rs"]);
    run(&["commit", "-m", "add new_file"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
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
        body.contains("new_file.rs"),
        "diff should mention new_file.rs"
    );
    assert!(
        body.contains("fn hello()"),
        "diff should contain the added content"
    );
}

#[tokio::test]
async fn run_diff_excludes_main_advance_after_fork() {
    // Three-dot range → commits landed on main AFTER the fork must not
    // appear as phantom deletions. RED under two-dot `git diff HEAD pdo/run-<id>`.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-fork";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let git = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };

    // The run's real work on the run branch.
    std::fs::write(wt_dir.join("run_file.rs"), "fn run_work() {}\n").unwrap();
    git(&wt_dir, &["add", "run_file.rs"]);
    git(&wt_dir, &["commit", "-m", "run work"]);

    // Advance main/HEAD AFTER the fork.
    std::fs::write(repo.join("main_advance.rs"), "fn advanced_on_main() {}\n").unwrap();
    git(repo, &["add", "main_advance.rs"]);
    git(repo, &["commit", "-m", "advance main after fork"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
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
        body.contains("run_file.rs") && body.contains("fn run_work()"),
        "diff must contain the run's own change: {body}"
    );
    // RED under two-dot (main_advance.rs shown as a deletion); GREEN under three-dot.
    assert!(
        !body.contains("main_advance.rs"),
        "diff must not mention main's post-fork file (phantom deletion): {body}"
    );
    assert!(
        !body.contains("advanced_on_main"),
        "diff must not contain main's post-fork content as a removed line: {body}"
    );
}

#[tokio::test]
async fn run_diff_ignores_parked_checkout_via_frozen_fork_sha() {
    // #417 (layer-2 mirror of the layer-3a `run_diff_range` test): with a `fork_sha`
    // frozen in `RunStarted`, `GET /runs/<id>/diff` must base on the fork point even
    // when the shared checkout's HEAD is parked on a divergent branch that forks
    // BEFORE it. RED under the old `HEAD...` range (the divergent branch's pre-fork
    // files render as a phantom); GREEN with `run_diff_base`.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let git = |dir: &std::path::Path, args: &[&str]| {
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
        out
    };
    let default_branch =
        String::from_utf8_lossy(&git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
            .trim()
            .to_string();

    // Divergent branch off c0 with a phantom-carrying commit; then advance the
    // default branch past the fork.
    git(repo, &["checkout", "-b", "wip/parked"]);
    std::fs::write(repo.join("phantom.rs"), "fn phantom() {}\n").unwrap();
    git(repo, &["add", "phantom.rs"]);
    git(repo, &["commit", "-m", "parked: unrelated work"]);
    git(repo, &["checkout", &default_branch]);
    std::fs::write(repo.join("main_advance.rs"), "fn advanced_on_main() {}\n").unwrap();
    git(repo, &["add", "main_advance.rs"]);
    git(repo, &["commit", "-m", "advance main after divergence"]);

    // Freeze the fork point = current default-branch tip.
    let fork_sha = String::from_utf8_lossy(&git(repo, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    let run_id = "diff-parked-fork";
    let state = test_state_with_dir(repo).await;
    // Seed a RunStarted carrying the frozen `fork_sha` (the fix's mechanism), plus a
    // node so the projection is realistic.
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
            "fork_sha": fork_sha,
            "node_defs": [
                { "id": "impl-1", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] }
            ],
            "edges": []
        })),
    };
    append_event(&state, &run_started).await.unwrap();

    // Cut the run branch/worktree from the fork point.
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, &default_branch).unwrap();

    // The run's only real change.
    std::fs::write(wt_dir.join("run_file.rs"), "fn run_work() {}\n").unwrap();
    git(&wt_dir, &["add", "run_file.rs"]);
    git(&wt_dir, &["commit", "-m", "run work"]);

    // PARK the shared checkout on the divergent branch (the #417 / #451 condition).
    git(repo, &["checkout", "wip/parked"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
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
        body.contains("run_file.rs") && body.contains("fn run_work()"),
        "diff must contain the run's own change: {body}"
    );
    // The phantom: the parked branch's pre-fork file. RED under `HEAD...`.
    assert!(
        !body.contains("phantom.rs"),
        "parked divergent HEAD must not add a phantom file to the diff: {body}"
    );
    assert!(
        !body.contains("main_advance.rs"),
        "main's post-fork file must not appear either (three-dot base): {body}"
    );
}

#[tokio::test]
async fn run_diff_excludes_pdo_artifacts() {
    // `:(exclude).pdo/` keeps the blackboard out of the diff. RED under
    // the current no-pathspec two-dot call.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "diff-pdo";
    let state = test_state_with_dir(repo).await;
    seed_run_with_node(&state, run_id, "impl-1", true).await;

    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&wt_dir)
            .output()
            .unwrap()
    };
    std::fs::write(wt_dir.join("code.rs"), "fn code_change() {}\n").unwrap();
    std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
    std::fs::write(wt_dir.join(".pdo/artifact.txt"), "internal-artifact\n").unwrap();
    git(&["add", "code.rs"]);
    // Force so it's genuinely committed → test is not vacuous
    git(&["add", "-f", ".pdo/artifact.txt"]);
    git(&["commit", "-m", "code + pdo artifact"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
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
        body.contains("code.rs") && body.contains("fn code_change()"),
        "diff should contain the real code change: {body}"
    );
    assert!(
        !body.contains("artifact.txt") && !body.contains("internal-artifact"),
        "diff must exclude .pdo/ artifacts: {body}"
    );
}

#[tokio::test]
async fn run_diff_computes_in_effective_repo_not_daemon_cwd() {
    // #748 AC1: the daemon's cwd has NO run branch; the Run's target repo has.
    // RED under `.current_dir(&state.repo_root)`: git answers "unknown
    // revision" → 404.
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let run_id = "diff-other-repo";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target_dir.path(), run_id).await;

    std::fs::write(wt_dir.join("elsewhere.rs"), "fn elsewhere() {}\n").unwrap();
    git_in(&wt_dir, &["add", "elsewhere.rs"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "work in target repo"]);

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("elsewhere.rs") && body.contains("fn elsewhere()"),
        "raw diff must come from the target repo: {body}"
    );
}

#[tokio::test]
async fn structured_diff_lists_files_hunks_and_matches_loc_bounds() {
    // #748 AC2: files + hunks, three-dot fork → tip, `.pdo/` excluded, and the
    // totals equal the LOC stat computed over the same bounds.
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "sdiff";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;

    // Modify README (1 del / 2 add), add a file (3 add), add a blackboard
    // artefact (excluded), add a binary (counted as a file, 0/0).
    std::fs::write(wt_dir.join("README.md"), "# test\n\nmore\n").unwrap();
    std::fs::write(wt_dir.join("src.rs"), "a\nb\nc\n").unwrap();
    std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
    std::fs::write(wt_dir.join(".pdo/artifact.txt"), "blackboard\n").unwrap();
    std::fs::write(wt_dir.join("blob.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["add", "-f", ".pdo/artifact.txt"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "run work"]);
    // Advance the target's main after the fork: three-dot must ignore it.
    std::fs::write(target.join("main_only.rs"), "fn main_only() {}\n").unwrap();
    git_in(target, &["add", "main_only.rs"]);
    git_in(target, &["commit", "-q", "-m", "main advance"]);

    // The events are what `load_projected` reads; compute LOC over the same
    // repo/bounds the endpoint must use.
    let events = load_events(&state.db, run_id).await.unwrap();
    let run_state = event_log::project(&events).unwrap();
    let loc = compute_run_loc(target, run_id, run_diff_base(&run_state)).expect("loc");

    let app = build_router(state);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();

    assert_eq!(json["three_dot"], serde_json::json!(true));
    // #835: the worktree exists, so the default destination is the working
    // tree; clean after the commit, its snapshot is HEAD's own tree and the
    // bounds equal fork → tip.
    assert_eq!(json["to_ref"], serde_json::json!("worktree"));
    assert_eq!(
        json["from_sha"],
        serde_json::json!(run_state.fork_sha.clone().unwrap())
    );
    assert_eq!(
        json["to_sha"].as_str().unwrap(),
        git_in(&wt_dir, &["rev-parse", "HEAD^{tree}"])
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured?from=fork&to=tip"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let tip_json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(
        tip_json["to_ref"],
        serde_json::json!(format!("pdo/run-{run_id}"))
    );
    assert_eq!(
        tip_json["to_sha"].as_str().unwrap(),
        git_in(&wt_dir, &["rev-parse", "HEAD"])
    );
    assert_eq!(tip_json["files"], json["files"]);

    assert_eq!(
        json["files_changed"].as_u64().unwrap(),
        loc.files_changed,
        "{json}"
    );
    assert_eq!(json["additions"].as_u64().unwrap(), loc.insertions);
    assert_eq!(json["deletions"].as_u64().unwrap(), loc.deletions);

    let files = json["files"].as_array().unwrap();
    let by_path = |p: &str| {
        files
            .iter()
            .find(|f| f["new_path"] == serde_json::json!(p))
            .unwrap_or_else(|| panic!("{p} missing in {json}"))
            .clone()
    };
    assert!(files
        .iter()
        .all(|f| f["new_path"] != serde_json::json!("main_only.rs")));
    assert!(files
        .iter()
        .all(|f| f["new_path"] != serde_json::json!(".pdo/artifact.txt")));

    let readme = by_path("README.md");
    assert_eq!(readme["status"], serde_json::json!("modified"));
    assert_eq!(readme["additions"], serde_json::json!(2));
    assert_eq!(readme["deletions"], serde_json::json!(0));
    let hunk = &readme["hunks"][0];
    assert_eq!(hunk["old_start"], serde_json::json!(1));
    let kinds: Vec<&str> = hunk["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["context", "add", "add"]);
    assert_eq!(hunk["lines"][2]["content"], serde_json::json!("more"));
    assert_eq!(hunk["lines"][2]["new_no"], serde_json::json!(3));
    assert!(hunk["lines"][2]["old_no"].is_null());

    let src = by_path("src.rs");
    assert_eq!(src["status"], serde_json::json!("added"));
    assert!(src["old_path"].is_null());
    assert_eq!(src["additions"], serde_json::json!(3));

    let bin = by_path("blob.bin");
    assert_eq!(bin["binary"], serde_json::json!(true));
    assert_eq!(bin["status"], serde_json::json!("added"));
    assert_eq!(bin["hunks"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn run_refs_lists_deliveries_and_stable_ids_resolve_on_diff_and_file() {
    // #749 AC1/AC5: the refs endpoint reads deliveries from the event log,
    // labels them by node and iter, and the stable ids resolve on the
    // structured diff and file-at-ref endpoints (URLs never carry SHAs).
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "refs-run";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
    let fork = git_in(target, &["rev-parse", "HEAD"]);
    let c0 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    std::fs::write(wt_dir.join("one.rs"), "1\n").unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "one"]);
    let c1 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    std::fs::write(wt_dir.join("two.rs"), "2\n").unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "two"]);
    let c2 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    // impl-1 delivered c0 → c1 at iter 1; a second node delivered c1 → c2.
    for (node, before, after) in [("impl-1", &c0, &c1), ("worker-2", &c1, &c2)] {
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeDelivered,
                node_id: Some(node.into()),
                iter: Some(1),
                payload: Some(serde_json::json!({ "before": before, "after": after })),
            },
        )
        .await
        .unwrap();
    }
    // impl-1 is still running and isolated: give it a live sub-worktree branch.
    let live_branch = worktrees::sub_worktree_branch(run_id, "impl-1", 1);
    git_in(target, &["branch", &live_branch, &c2]);

    let app = build_router(state);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/refs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let ids: Vec<&str> = json["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "fork",
            "node:impl-1:1:before",
            "node:impl-1:1:after",
            "node:worker-2:1:before",
            "node:worker-2:1:after",
            "live:impl-1",
            "tip",
            "worktree",
        ]
    );
    let refs = json["refs"].as_array().unwrap();
    assert_eq!(refs[0]["sha"], serde_json::json!(fork));
    assert_eq!(refs[2]["sha"], serde_json::json!(c1));
    assert_eq!(
        refs[2]["label"],
        serde_json::json!("impl-1 · iter 1 · after")
    );
    assert_eq!(refs[5]["sha"], serde_json::json!(c2));
    assert_eq!(refs[6]["sha"], serde_json::json!(c2));
    // #835: the working tree is a ref while the worktree exists — clean here,
    // so its snapshot is HEAD's tree — and the default destination.
    assert_eq!(refs[7]["kind"], serde_json::json!("worktree"));
    assert_eq!(refs[7]["label"], serde_json::json!("Working tree"));
    assert_eq!(
        refs[7]["sha"].as_str().unwrap(),
        git_in(&wt_dir, &["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(json["default_from"], serde_json::json!("fork"));
    assert_eq!(json["default_to"], serde_json::json!("worktree"));
    let deliveries = json["deliveries"].as_array().unwrap();
    assert_eq!(deliveries.len(), 3);
    assert_eq!(deliveries[0]["status"], serde_json::json!("delivered"));
    assert_eq!(deliveries[2]["status"], serde_json::json!("running"));
    assert_eq!(deliveries[2]["live"], serde_json::json!("live:impl-1"));

    // The delivery pair, by id: only impl-1's file, two-dot.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/diff/structured?from=node:impl-1:1:before&to=node:impl-1:1:after"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["three_dot"], serde_json::json!(false));
    assert_eq!(json["from_sha"], serde_json::json!(c0));
    assert_eq!(json["to_sha"], serde_json::json!(c1));
    let paths: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["new_path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["one.rs"]);

    // `fork` → `tip` spelled out is the three-dot default.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured?from=fork&to=tip"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["three_dot"], serde_json::json!(true));
    assert_eq!(json["files_changed"], serde_json::json!(2));

    // tip → live branch resolves too.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/diff/structured?from=tip&to=live:impl-1"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // An id the Run does not know is a 404, never handed to git.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/diff/structured?from=node:impl-1:2:before"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(body_string(resp).await.contains("unknown run ref"));

    // The file endpoint resolves ids as well.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/file?path=one.rs&ref=node:impl-1:1:after"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "1\n");
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/file?path=one.rs&ref=node:impl-1:1:before"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "one.rs did not exist before"
    );
}

#[tokio::test]
async fn default_review_diff_shows_the_dirty_worktree_of_a_running_run() {
    // #835: with no query, the structured diff is fork → working tree: a
    // modified tracked file and an untracked file show up while nothing is
    // committed; the file endpoint reads the file on disk; a comment
    // anchored on the working tree freezes the snapshot tree id; and once
    // the worktree is gone, the default falls back to fork → tip.
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "dirty-run";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
    let head = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    std::fs::write(wt_dir.join("README.md"), "# test\n\nuncommitted\n").unwrap();
    std::fs::write(wt_dir.join("REPRO_835.md"), "untracked\n").unwrap();
    std::fs::create_dir_all(wt_dir.join(".pdo/artifacts")).unwrap();
    std::fs::write(wt_dir.join(".pdo/artifacts/out.md"), "blackboard\n").unwrap();
    // The documented target-repo setup (ADR-0060) ignores `.pdo/` as a
    // directory: the snapshot must not trip on it (#835 FP finding).
    let common_dir = git_in(
        &wt_dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    std::fs::create_dir_all(Path::new(&common_dir).join("info")).unwrap();
    std::fs::write(Path::new(&common_dir).join("info/exclude"), ".pdo/\n").unwrap();
    let status_before = git_in(&wt_dir, &["status", "--porcelain"]);

    let app = build_router(state);
    let get = |uri: String| {
        app.clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
    };

    let resp = get(format!("/runs/{run_id}/diff/structured"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["to_ref"], serde_json::json!("worktree"));
    assert_eq!(json["three_dot"], serde_json::json!(true));
    let mut paths: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["new_path"].as_str().unwrap())
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["README.md", "REPRO_835.md"], "{json}");
    let snapshot = json["to_sha"].as_str().unwrap().to_string();
    assert_ne!(snapshot, head);
    // The run branch itself has not moved: fork → tip is still empty.
    let resp = get(format!("/runs/{run_id}/diff/structured?to=tip"))
        .await
        .unwrap();
    let tip: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(tip["files_changed"], serde_json::json!(0));
    // Reading the diff moved nothing for the node.
    assert_eq!(git_in(&wt_dir, &["status", "--porcelain"]), status_before);
    assert_eq!(git_in(&wt_dir, &["rev-parse", "HEAD"]), head);

    // The working tree is the destination only.
    let resp = get(format!(
        "/runs/{run_id}/diff/structured?from=worktree&to=tip"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // File at the default ref = the file on disk, untracked included.
    let resp = get(format!("/runs/{run_id}/file?path=REPRO_835.md"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "untracked\n");
    let resp = get(format!("/runs/{run_id}/file?path=REPRO_835.md&ref=tip"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = get(format!("/runs/{run_id}/file?path=nope.md&ref=worktree"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // The refs endpoint lists it with the same snapshot id.
    let resp = get(format!("/runs/{run_id}/refs")).await.unwrap();
    let refs: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let wt = refs["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == serde_json::json!("worktree"))
        .expect("worktree ref listed");
    assert_eq!(wt["sha"], serde_json::json!(snapshot));
    assert_eq!(refs["default_to"], serde_json::json!("worktree"));

    // Worktree gone (finished + cleaned, or archived): back to fork → tip.
    git_in(
        target,
        &["worktree", "remove", "--force", wt_dir.to_str().unwrap()],
    );
    let resp = get(format!("/runs/{run_id}/refs")).await.unwrap();
    let refs: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(refs["refs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["id"] != serde_json::json!("worktree")));
    assert_eq!(refs["default_to"], serde_json::json!("tip"));
    let resp = get(format!("/runs/{run_id}/diff/structured"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(
        json["to_ref"],
        serde_json::json!(format!("pdo/run-{run_id}"))
    );
    let resp = get(format!("/runs/{run_id}/diff/structured?to=worktree"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn structured_diff_explicit_pair_is_two_dot_and_unsafe_ref_is_400() {
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "sdiff-pair";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
    let c0 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    std::fs::write(wt_dir.join("one.rs"), "1\n").unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "one"]);
    let c1 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
    std::fs::write(wt_dir.join("two.rs"), "2\n").unwrap();
    git_in(&wt_dir, &["add", "-A"]);
    git_in(&wt_dir, &["commit", "-q", "-m", "two"]);

    let app = build_router(state);
    // A delivery-like pair (c0 → c1) shows only `one.rs`.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured?from={c0}&to={c1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["three_dot"], serde_json::json!(false));
    assert_eq!(json["files_changed"], serde_json::json!(1));
    assert_eq!(json["files"][0]["new_path"], serde_json::json!("one.rs"));

    // Option injection is refused before git ever runs.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/runs/{run_id}/diff/structured?from=--output%3D%2Ftmp%2Fx"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // An unknown ref is a 404, like the raw endpoint.
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured?to=no-such-branch"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn structured_diff_is_empty_when_no_changes_and_404_when_branch_gone() {
    let daemon_dir = tempfile::tempdir().unwrap();
    init_test_repo(daemon_dir.path());
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path();
    let run_id = "sdiff-empty";
    let state = test_state_with_dir(daemon_dir.path()).await;
    let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;

    let app = build_router(state);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["files_changed"], serde_json::json!(0));
    assert_eq!(json["files"].as_array().unwrap().len(), 0);

    // Cleanup deletes the run branch (ADR-0020): nothing left to diff.
    git_in(
        target,
        &["worktree", "remove", "--force", wt_dir.to_str().unwrap()],
    );
    git_in(target, &["branch", "-D", &format!("pdo/run-{run_id}")]);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/diff/structured"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
