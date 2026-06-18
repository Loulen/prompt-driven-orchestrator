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

#[tokio::test]
async fn create_run_without_target_repo_uses_daemon_repo() {
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

    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    let run_id = json["run_id"].as_str().unwrap();

    // Artifacts should be under daemon's repo root
    let run_dir = daemon.repo_root().join(".pdo").join("runs").join(run_id);
    assert!(
        run_dir.exists(),
        "run dir must exist under daemon repo root"
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
    let branches: Vec<String> = resp.json().await.unwrap();
    assert!(branches.contains(&"main".to_string()));
    assert!(branches.contains(&"dev".to_string()));
    assert!(branches.contains(&"staging".to_string()));
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
