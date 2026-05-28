mod common;

use std::process::Command;

use common::TestDaemon;

const PIPELINE_NAME: &str = "recent-repos-test";
const PIPELINE_YAML: &str = r#"name: recent-repos-test
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

fn seed_daemon_repo(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
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

async fn create_run_with_target(daemon: &TestDaemon, target_repo: &str) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": target_repo,
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201, "POST /runs should succeed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn get_recent_repos(daemon: &TestDaemon) -> Vec<String> {
    let resp = reqwest::get(format!("{}/repos/recent", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json().await.unwrap()
}

#[tokio::test]
async fn recent_repos_empty_before_any_runs() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();
    let repos = get_recent_repos(&daemon).await;
    assert!(repos.is_empty());
}

#[tokio::test]
async fn recent_repos_returns_repos_in_order_and_deduplicates() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let target_a = tempfile::tempdir().unwrap();
    git_init_with_commit(target_a.path()).unwrap();
    let target_b = tempfile::tempdir().unwrap();
    git_init_with_commit(target_b.path()).unwrap();

    let path_a = target_a.path().to_str().unwrap();
    let path_b = target_b.path().to_str().unwrap();

    create_run_with_target(&daemon, path_a).await;
    create_run_with_target(&daemon, path_b).await;
    create_run_with_target(&daemon, path_a).await;

    let repos = get_recent_repos(&daemon).await;
    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0], path_a, "most recent repo should be first");
    assert_eq!(repos[1], path_b);
}
