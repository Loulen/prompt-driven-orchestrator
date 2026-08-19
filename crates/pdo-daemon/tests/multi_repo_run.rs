//! Layer 3a — proves issue #114: multi-repo run creation with target_repo and
//! source_branch selection. Validates:
//! - POST /runs accepts and validates target_repo (rejects non-git dirs)
//! - POST /runs accepts and validates source_branch (rejects missing branches)
//! - create_worktree branches from the selected source_branch
//! - Run artifacts live under <target_repo>/.pdo/runs/<run-id>/
//! - GET /repos/branches returns branches for a given repo path

mod common;

use std::process::Command;

use common::TestDaemon;

const PIPELINE_NAME: &str = "multi-repo-test";
const PIPELINE_YAML: &str = r#"name: multi-repo-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 100, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-b", "main"])?;
    run(&["config", "user.email", "test@test.com"])?;
    run(&["config", "user.name", "Test"])?;
    std::fs::write(repo.join("README.md"), "test")?;
    run(&["add", "."])?;
    run(&["commit", "-m", "init"])?;
    Ok(())
}

fn git_create_branch(repo: &std::path::Path, branch: &str) -> anyhow::Result<()> {
    let out = Command::new("git")
        .args(["branch", branch])
        .current_dir(repo)
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git branch {} failed: {}",
            branch,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Stage a `work` clone (at `work`) of a bare origin (at `origin`) carrying a
/// **remote-only** branch and the dedup case — all over a filesystem path, zero
/// network (#571). After this, `work` holds: local `main` + `local-branch`;
/// remote-tracking `origin/main` (twin of local `main` → deduped) and
/// `origin/feature-remote-only` (remote-only → must surface). Mirrors the FP
/// staging so the layer-3a and layer-5 fixtures stay in step.
fn stage_remote_repo(origin: &std::path::Path, work: &std::path::Path) -> anyhow::Result<()> {
    let run = |dir: &std::path::Path, args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(dir).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} in {} failed: {}",
                args.join(" "),
                dir.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(origin, &["init", "--bare", "-b", "main"])?;
    run(
        work,
        &["clone", origin.to_str().unwrap(), work.to_str().unwrap()],
    )?;
    run(work, &["config", "user.email", "test@test.com"])?;
    run(work, &["config", "user.name", "Test"])?;
    std::fs::write(work.join("README.md"), "hi")?;
    run(work, &["add", "."])?;
    run(work, &["commit", "-m", "init"])?;
    run(work, &["push", "-u", "origin", "main"])?;
    run(work, &["checkout", "-b", "feature-remote-only"])?;
    std::fs::write(work.join("x.txt"), "x")?;
    run(work, &["add", "."])?;
    run(work, &["commit", "-m", "feat"])?;
    run(work, &["push", "-u", "origin", "feature-remote-only"])?;
    run(work, &["checkout", "main"])?;
    // Drop the local branch: `feature-remote-only` now exists ONLY as a tracking ref.
    run(work, &["branch", "-D", "feature-remote-only"])?;
    run(work, &["checkout", "-b", "local-branch"])?;
    run(work, &["commit", "-m", "empty", "--allow-empty"])?;
    run(work, &["checkout", "main"])?;
    Ok(())
}

fn seed_daemon_repo(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are a worker.")?;
    git_init_with_commit(repo)?;
    Ok(())
}

// --- #465 slice 2 helpers: mid-run repo-list edit -----------------------------

/// Create a mono-repo Run against the daemon's own repo and return its id. The Run
/// is `running` the moment `POST /runs` returns (RunStarted is appended before the
/// response), so a `PATCH …/repos` right after edits a LIVE Run.
async fn create_mono_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "mono-repo create must succeed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn patch_repos(
    daemon: &TestDaemon,
    run_id: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .patch(format!("{}/runs/{}/repos", daemon.url(), run_id))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// A fresh git repo (one commit) at `dir`, ready to be pinned as a secondary.
fn make_secondary_repo(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    git_init_with_commit(dir).unwrap();
}

/// `<primary>/.pdo/runs/<run_id>/repos/<alias>/` — the on-disk snapshot location.
fn snapshot_dir(daemon: &TestDaemon, run_id: &str, alias: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("repos")
        .join(alias)
}

/// How many worktrees `secondary` has registered (`worktree list --porcelain` counts
/// `worktree ` records): 1 = just the repo itself, 2 = repo + one snapshot.
fn registered_worktree_count(secondary: &std::path::Path) -> usize {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(secondary)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .matches("worktree ")
        .count()
}

// --- Tests ---

#[tokio::test]
async fn create_run_rejects_nonexistent_target_repo() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": "/nonexistent/path/to/repo",
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("does not exist"),
        "error should mention path not existing: {:?}",
        json
    );
}

#[tokio::test]
async fn create_run_rejects_non_git_target_repo() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let non_git_dir = tempfile::tempdir().unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": non_git_dir.path().to_str().unwrap(),
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("not a git repository"),
        "error should mention not a git repo: {:?}",
        json
    );
}

#[tokio::test]
async fn create_run_rejects_relative_target_repo() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": "relative/path",
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("absolute path"),
        "error should mention absolute path: {:?}",
        json
    );
}

#[tokio::test]
async fn create_run_rejects_nonexistent_source_branch() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        // #470: the target-repo check runs FIRST, so this test must name one —
        // otherwise it 400s for the wrong reason and stops testing the branch.
        "target_repo": daemon.target_repo(),
        "source_branch": "nonexistent-branch",
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("does not exist"),
        "error should mention branch not existing: {:?}",
        json
    );
}

#[tokio::test]
async fn create_run_with_valid_target_repo_and_source_branch() {
    let target_repo = tempfile::tempdir().unwrap();
    git_init_with_commit(target_repo.path()).unwrap();
    git_create_branch(target_repo.path(), "feature-branch").unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": target_repo.path().to_str().unwrap(),
        "source_branch": "feature-branch",
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        201,
        "POST /runs should succeed with valid target_repo and source_branch"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    let run_id = json["run_id"].as_str().unwrap();

    // Artifacts should be under <target_repo>/.pdo/runs/<run-id>/
    let run_dir = target_repo.path().join(".pdo").join("runs").join(run_id);
    assert!(run_dir.exists(), "run dir must exist under target_repo");

    let worktree_dir = run_dir.join("worktree");
    assert!(worktree_dir.exists(), "worktree must exist");

    // Verify worktree was branched from feature-branch, not HEAD
    let output = Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(&worktree_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    // The run state should include target_repo and source_branch
    let run_resp = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap();
    assert_eq!(run_resp.status(), 200);
    let run_state: serde_json::Value = run_resp.json().await.unwrap();
    assert_eq!(
        run_state["target_repo"].as_str().unwrap(),
        target_repo.path().to_str().unwrap()
    );
    assert_eq!(
        run_state["source_branch"].as_str().unwrap(),
        "feature-branch"
    );
}

/// #470/AC1, layer 3a — the exact reproduction of the 2026-07-29 incident, in
/// which two Runs wrote code into `~/.pdo/app` (the daemon's own working
/// directory) because nobody had named a repo. That directory is no longer an
/// implicit Run target: the request is refused, and nothing at all is created.
///
/// ADR-0004's rule of thumb ("no AC closed without a layer ≥ 3 test") makes this
/// the load-bearing test of the change — the inline `create_run_without_target_repo_is_400`
/// exercises the same boundary, but only this one runs a real daemon against a
/// real git repo and can prove no worktree appeared on disk.
#[tokio::test]
async fn create_run_without_target_repo_is_refused() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "a Run that names no target repo must be refused, not silently \
         redirected at the daemon's own working directory"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("target_repo"),
        "the error must name the field: {json:?}"
    );

    // Nothing was created: no Run in the list...
    let runs: Vec<serde_json::Value> = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        runs.is_empty(),
        "no Run may be created on refusal: {runs:?}"
    );

    // ...and — the incident itself — no worktree scaffolding under the daemon root.
    // The `.pdo/runs` directory itself exists from boot (the pipeline watcher
    // creates it to watch run-scoped edits), so what must be empty is its contents.
    let runs_dir = daemon.repo_root().join(".pdo").join("runs");
    let entries: Vec<_> = std::fs::read_dir(&runs_dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        entries.is_empty(),
        "no run dir may appear under the daemon's own repo root, found: {entries:?}"
    );
}

#[tokio::test]
async fn list_branches_endpoint_returns_branches() {
    let target_repo = tempfile::tempdir().unwrap();
    git_init_with_commit(target_repo.path()).unwrap();
    git_create_branch(target_repo.path(), "dev").unwrap();
    git_create_branch(target_repo.path(), "staging").unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let repo_path = target_repo.path().to_str().unwrap();
    let resp = reqwest::Client::new()
        .get(format!("{}/repos/branches", daemon.url()))
        .query(&[("path", repo_path)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    // The payload is `[{name, kind}]` now (#571), not a flat `string[]`.
    let branches: Vec<serde_json::Value> = resp.json().await.unwrap();
    let named = |n: &str| {
        branches
            .iter()
            .find(|b| b["name"].as_str() == Some(n))
            .unwrap_or_else(|| panic!("branch {n} missing from {branches:?}"))
    };
    assert_eq!(named("main")["kind"].as_str(), Some("local"));
    assert_eq!(named("dev")["kind"].as_str(), Some("local"));
    assert_eq!(named("staging")["kind"].as_str(), Some("local"));
}

#[tokio::test]
async fn list_branches_endpoint_returns_remote_branches_with_kind() {
    let origin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    stage_remote_repo(origin.path(), work.path()).unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let resp = reqwest::Client::new()
        .get(format!("{}/repos/branches", daemon.url()))
        .query(&[("path", work.path().to_str().unwrap())])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let branches: Vec<serde_json::Value> = resp.json().await.unwrap();

    let kind_of = |n: &str| -> Option<String> {
        branches
            .iter()
            .find(|b| b["name"].as_str() == Some(n))
            .map(|b| b["kind"].as_str().unwrap_or_default().to_string())
    };

    // Locals surface as local; the remote-only branch surfaces as remote.
    assert_eq!(kind_of("main").as_deref(), Some("local"), "{branches:?}");
    assert_eq!(
        kind_of("local-branch").as_deref(),
        Some("local"),
        "{branches:?}"
    );
    assert_eq!(
        kind_of("origin/feature-remote-only").as_deref(),
        Some("remote"),
        "{branches:?}"
    );

    // origin/main (twin of local main) is deduped; the symref never appears.
    assert!(
        kind_of("origin/main").is_none(),
        "dedup failed: {branches:?}"
    );
    assert!(
        kind_of("origin/HEAD").is_none(),
        "symref leaked: {branches:?}"
    );
    assert!(
        kind_of("origin").is_none(),
        "bare origin leaked: {branches:?}"
    );

    // Every local precedes every remote.
    let first_remote = branches
        .iter()
        .position(|b| b["kind"].as_str() == Some("remote"));
    let last_local = branches
        .iter()
        .rposition(|b| b["kind"].as_str() == Some("local"));
    assert!(
        matches!((first_remote, last_local), (Some(fr), Some(ll)) if ll < fr),
        "locals must precede remotes: {branches:?}"
    );
}

/// The load-bearing test of #571 (ADR-0004: no AC closed without a layer ≥ 3
/// test). A Run cut from a branch that exists ONLY as a remote-tracking ref must
/// succeed — this was the 400 bug — with the worktree branched from that ref and
/// NO local branch materialised, and NO fetch.
#[tokio::test]
async fn create_run_from_a_remote_only_branch_creates_the_worktree() {
    let origin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    stage_remote_repo(origin.path(), work.path()).unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": work.path().to_str().unwrap(),
        "source_branch": "origin/feature-remote-only",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "a Run from a remote-only branch must be accepted"
    );
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The worktree was cut from origin/feature-remote-only: its tip is that ref's tip.
    let worktree = work
        .path()
        .join(".pdo")
        .join("runs")
        .join(&run_id)
        .join("worktree");
    assert!(worktree.exists(), "worktree must exist");
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    let ref_tip = Command::new("git")
        .args(["rev-parse", "origin/feature-remote-only"])
        .current_dir(work.path())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        String::from_utf8_lossy(&ref_tip.stdout).trim(),
        "worktree HEAD must equal the remote-tracking ref's tip"
    );

    // NO local branch `feature-remote-only` was materialised in the target repo.
    let local = Command::new("git")
        .args(["branch", "--list", "feature-remote-only"])
        .current_dir(work.path())
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&local.stdout).trim().is_empty(),
        "no local branch may be created from a remote-only source"
    );

    // The source_branch is stored verbatim, prefix and all.
    let run_state: serde_json::Value = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        run_state["source_branch"].as_str(),
        Some("origin/feature-remote-only")
    );
}

#[tokio::test]
async fn create_run_rejects_unknown_remote_branch() {
    let origin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    stage_remote_repo(origin.path(), work.path()).unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": work.path().to_str().unwrap(),
        "source_branch": "origin/nope",
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    let err = json["error"].as_str().unwrap_or_default();
    assert!(err.contains("does not exist"), "message was: {err}");
    assert!(
        err.contains("origin/nope"),
        "message must name the ref: {err}"
    );
}

#[tokio::test]
async fn list_branches_rejects_non_git_path() {
    let non_git_dir = tempfile::tempdir().unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let repo_path = non_git_dir.path().to_str().unwrap();
    let resp = reqwest::Client::new()
        .get(format!("{}/repos/branches", daemon.url()))
        .query(&[("path", repo_path)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn validate_repo_endpoint_validates_git_repo() {
    let target_repo = tempfile::tempdir().unwrap();
    git_init_with_commit(target_repo.path()).unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let repo_path = target_repo.path().to_str().unwrap();
    let resp = reqwest::Client::new()
        .get(format!("{}/repos/validate", daemon.url()))
        .query(&[("path", repo_path)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn validate_repo_endpoint_rejects_non_git() {
    let non_git_dir = tempfile::tempdir().unwrap();

    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let repo_path = non_git_dir.path().to_str().unwrap();
    let resp = reqwest::Client::new()
        .get(format!("{}/repos/validate", daemon.url()))
        .query(&[("path", repo_path)])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["valid"], false);
    assert!(json["error"].as_str().is_some());
}

// --- #465 slice 2: mid-run edit of the secondary list -------------------------

/// A live mono-repo Run grows a secondary: the pin projects, the snapshot is on disk
/// at the frozen SHA with a detached HEAD, and the secondary repo registers it.
#[tokio::test]
async fn edit_add_secondary_mid_run() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());

    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": secondary.path().to_str().unwrap() }] }),
    )
    .await;
    assert_eq!(
        resp.status(),
        200,
        "adding a secondary to a live Run must succeed"
    );

    let state: serde_json::Value = resp.json().await.unwrap();
    let repos = state["target_repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1, "the projection must carry the new pin");
    let alias = repos[0]["alias"].as_str().unwrap();
    let sha = repos[0]["sha"].as_str().unwrap();
    assert!(!sha.is_empty());

    let snap = snapshot_dir(&daemon, &run_id, alias);
    assert!(
        snap.exists(),
        "the snapshot dir must be materialised on disk"
    );

    // Detached HEAD, pinned at the frozen SHA.
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&snap)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), sha);

    // The secondary now has two registered worktrees (itself + the detached snapshot).
    assert_eq!(
        registered_worktree_count(secondary.path()),
        2,
        "the secondary must register the snapshot worktree"
    );
    let list = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(secondary.path())
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("detached"),
        "the snapshot worktree must be detached (no branch)"
    );
}

/// ADR-0047: the `read_only` opt-in survives the create/edit chokepoint and the
/// `RunReposEdited` projection — a `read_only: true` pin comes back flagged, a pin
/// added without the key comes back writable (default). This is the data-layer
/// round-trip of the whole feature, through the real HTTP handler.
#[tokio::test]
async fn edit_add_secondary_carries_read_only_flag() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let ro = tempfile::tempdir().unwrap();
    let rw = tempfile::tempdir().unwrap();
    make_secondary_repo(ro.path());
    make_secondary_repo(rw.path());

    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({
            "add": [
                { "repo": ro.path().to_str().unwrap(), "read_only": true },
                { "repo": rw.path().to_str().unwrap() }
            ]
        }),
    )
    .await;
    assert_eq!(resp.status(), 200, "adding secondaries must succeed");

    let state: serde_json::Value = resp.json().await.unwrap();
    let repos = state["target_repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2, "both pins must project");

    // The read-only pin carries the flag; the default one omits it (writable).
    let ro_pin = repos
        .iter()
        .find(|p| p["repo"] == serde_json::json!(ro.path().to_str().unwrap()))
        .expect("read-only pin present");
    let rw_pin = repos
        .iter()
        .find(|p| p["repo"] == serde_json::json!(rw.path().to_str().unwrap()))
        .expect("writable pin present");
    assert_eq!(
        ro_pin["read_only"],
        serde_json::json!(true),
        "the opted-in pin must project read_only=true"
    );
    assert!(
        rw_pin.get("read_only").is_none(),
        "a writable pin must omit read_only on the wire (serde skip), got {rw_pin}"
    );
}

/// Removing a secondary drops it from the projection but LEAVES the snapshot on disk
/// (deferred teardown, decision 5) — a still-live reader keeps a valid path.
#[tokio::test]
async fn edit_remove_secondary_mid_run() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());

    let add = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": secondary.path().to_str().unwrap() }] }),
    )
    .await;
    let added: serde_json::Value = add.json().await.unwrap();
    let alias = added["target_repos"][0]["alias"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = snapshot_dir(&daemon, &run_id, &alias);
    assert!(snap.exists());

    let remove = patch_repos(&daemon, &run_id, serde_json::json!({ "remove": [alias] })).await;
    assert_eq!(remove.status(), 200);
    let state: serde_json::Value = remove.json().await.unwrap();
    assert!(
        state["target_repos"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "the removed secondary must be gone from the projection"
    );

    assert!(
        snap.exists(),
        "the snapshot must remain on disk after removal — teardown is deferred to cleanup"
    );
}

/// #221 (handler half): a terminal (archived) Run's list is frozen — the edit is
/// refused 409 `run_not_editable`, never applied.
#[tokio::test]
async fn edit_rejects_terminal_run() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    // Archive it (terminal, but NOT forgotten — so it reaches the terminal guard, not
    // the 410 tombstone guard).
    let cleanup = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(
        cleanup.status().is_success(),
        "cleanup_run must archive the Run"
    );

    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());

    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": secondary.path().to_str().unwrap() }] }),
    )
    .await;
    assert_eq!(resp.status(), 409, "a terminal Run is not editable");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "run_not_editable");
}

/// The `add` refusals: the primary (self-reference) and an already-pinned repo.
#[tokio::test]
async fn edit_rejects_primary_and_duplicate() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    // Adding the primary as its own secondary → 400.
    let self_ref = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": daemon.target_repo() }] }),
    )
    .await;
    assert_eq!(self_ref.status(), 400);
    let body: serde_json::Value = self_ref.json().await.unwrap();
    assert_eq!(body["error"], "secondary_is_primary");

    // Adding the same secondary that is already pinned → 409.
    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());
    let path = secondary.path().to_str().unwrap();
    let first = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": path }] }),
    )
    .await;
    assert_eq!(first.status(), 200);

    let dup = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": path }] }),
    )
    .await;
    assert_eq!(dup.status(), 409);
    let body: serde_json::Value = dup.json().await.unwrap();
    assert_eq!(body["error"], "secondary_already_pinned");
}

/// A bad base branch fails fast: 400 `bad_secondary_repo`, no snapshot, no change to
/// the projection — and never a git fetch.
#[tokio::test]
async fn edit_bad_branch_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());

    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({
            "add": [{ "repo": secondary.path().to_str().unwrap(), "base_branch": "nope-not-a-branch" }]
        }),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "bad_secondary_repo");

    // No snapshot was materialised, and the projection is still mono-repo.
    let repos_dir = daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(&run_id)
        .join("repos");
    let has_any = std::fs::read_dir(&repos_dir)
        .map(|rd| rd.flatten().next().is_some())
        .unwrap_or(false);
    assert!(!has_any, "a failed add must leave no snapshot on disk");

    let state: serde_json::Value = reqwest::get(format!("{}/runs/{}", daemon.url(), run_id))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(state["target_repos"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));
}

/// Removing an alias that is not pinned is an idempotent no-op (200, list unchanged)
/// — the UI removes by displayed alias and a double-click must not 404.
#[tokio::test]
async fn edit_remove_absent_alias_is_noop() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "remove": ["ghost-alias"] }),
    )
    .await;
    assert_eq!(
        resp.status(),
        200,
        "removing an absent alias is a no-op, not a 404"
    );
    let state: serde_json::Value = resp.json().await.unwrap();
    assert!(state["target_repos"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));
}

/// An empty body is a legal no-op 200.
#[tokio::test]
async fn edit_empty_body_is_noop() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let resp = patch_repos(&daemon, &run_id, serde_json::json!({})).await;
    assert_eq!(resp.status(), 200);
}

/// A forgotten (tombstoned) Run refuses the edit 410 before any other check.
#[tokio::test]
async fn edit_forgotten_run_is_410() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    // A run must be archived before it can be forgotten (tombstoned).
    let cleanup = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(cleanup.status().is_success());
    let forget = reqwest::Client::new()
        .delete(format!("{}/runs/{}", daemon.url(), run_id))
        .send()
        .await
        .unwrap();
    assert!(forget.status().is_success());

    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());
    let resp = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": secondary.path().to_str().unwrap() }] }),
    )
    .await;
    assert_eq!(resp.status(), 410);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "run_forgotten");
}

/// Alias disambiguation is seeded from disk: `add repoB` → `remove` → `add another
/// repoB` gets a DISTINCT alias, because the removed-but-persistent snapshot still
/// owns its folder name.
#[tokio::test]
async fn alias_collision_after_remove_readd() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    // Two DIFFERENT repos that share the basename "libB".
    let a = tempfile::tempdir().unwrap();
    let repo_a = a.path().join("libB");
    make_secondary_repo(&repo_a);
    let b = tempfile::tempdir().unwrap();
    let repo_b = b.path().join("libB");
    make_secondary_repo(&repo_b);

    let add_a = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": repo_a.to_str().unwrap() }] }),
    )
    .await;
    let state_a: serde_json::Value = add_a.json().await.unwrap();
    assert_eq!(state_a["target_repos"][0]["alias"], "libB");

    // Remove it — the snapshot folder `repos/libB` persists on disk.
    let remove = patch_repos(&daemon, &run_id, serde_json::json!({ "remove": ["libB"] })).await;
    assert_eq!(remove.status(), 200);

    // Re-add the OTHER repoB: the disk-seeded disambiguation must avoid `libB`.
    let add_b = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": repo_b.to_str().unwrap() }] }),
    )
    .await;
    assert_eq!(add_b.status(), 200);
    let state_b: serde_json::Value = add_b.json().await.unwrap();
    assert_eq!(
        state_b["target_repos"][0]["alias"], "libB-2",
        "the second repoB must not reuse the removed snapshot's folder name"
    );
    assert!(snapshot_dir(&daemon, &run_id, "libB").exists());
    assert!(snapshot_dir(&daemon, &run_id, "libB-2").exists());
}

/// `cleanup_run` is disk-driven: it prunes EVERY snapshot under `repos/*` — the
/// active one AND the removed-but-persistent one — from its owning secondary, so no
/// dangling `--detach` registration survives (anti-#498).
#[tokio::test]
async fn cleanup_removes_all_snapshots_disk_scan() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let run_id = create_mono_run(&daemon).await;

    let active = tempfile::tempdir().unwrap();
    make_secondary_repo(active.path());
    let removed = tempfile::tempdir().unwrap();
    make_secondary_repo(removed.path());

    // Pin both, then remove one (its snapshot persists on disk, off-projection).
    let add1 = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": active.path().to_str().unwrap() }] }),
    )
    .await;
    assert_eq!(add1.status(), 200);
    let add2 = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "add": [{ "repo": removed.path().to_str().unwrap() }] }),
    )
    .await;
    let state2: serde_json::Value = add2.json().await.unwrap();
    let removed_alias = state2["target_repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["repo"] == serde_json::json!(removed.path().to_str().unwrap()))
        .unwrap()["alias"]
        .as_str()
        .unwrap()
        .to_string();
    let drop = patch_repos(
        &daemon,
        &run_id,
        serde_json::json!({ "remove": [removed_alias] }),
    )
    .await;
    assert_eq!(drop.status(), 200);

    // Both secondaries currently register their snapshot.
    assert_eq!(registered_worktree_count(active.path()), 2);
    assert_eq!(registered_worktree_count(removed.path()), 2);

    // Archive — the disk scan must prune BOTH.
    let cleanup = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(cleanup.status().is_success());

    assert!(
        !daemon
            .repo_root()
            .join(".pdo")
            .join("runs")
            .join(&run_id)
            .exists(),
        "the run dir must be gone after cleanup"
    );
    assert_eq!(
        registered_worktree_count(active.path()),
        1,
        "the active secondary's snapshot registration must be pruned"
    );
    assert_eq!(
        registered_worktree_count(removed.path()),
        1,
        "the removed-but-persistent secondary's registration must be pruned too"
    );
}
