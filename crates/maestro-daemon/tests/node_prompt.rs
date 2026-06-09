//! Layer 3a — node_prompt endpoint integration test for issue #26.
//!
//! Spawns a real TestDaemon, creates a run via POST /runs, then asserts the
//! GET /runs/{run_id}/nodes/{node_id}/prompt endpoint returns the augmented
//! prompt containing deterministic ## Inputs and ## Outputs sections.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "prompt-test";
const NODE_ID: &str = "worker";
const PIPELINE_YAML: &str = r#"name: prompt-test
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
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

const ROLE_PROMPT: &str = "You are a worker. Do the task.\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;

    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), ROLE_PROMPT)?;

    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "test@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".maestro/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon_url: &str) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
    });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

/// Layer 3a: POST /runs spawns nodes, writing the augmented prompt to disk.
/// GET /runs/{run_id}/nodes/{node_id}/prompt returns it as text/markdown
/// with the deterministic preamble sections.
#[tokio::test]
async fn prompt_endpoint_returns_augmented_prompt_after_run_creation() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let resp = reqwest::get(format!(
        "{}/runs/{}/nodes/{}/prompt?iter=1",
        daemon.url(),
        run_id,
        NODE_ID,
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/markdown"
    );

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("## Inputs"),
        "prompt must contain ## Inputs section, got: {body}"
    );
    assert!(
        body.contains("## Outputs"),
        "prompt must contain ## Outputs section, got: {body}"
    );
    assert!(
        body.contains(ROLE_PROMPT.trim()),
        "prompt must contain the role prompt, got: {body}"
    );

    // Clean up tmux session if it was created
    let session_name = format!("maestro-{run_id}-{NODE_ID}-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session_name])
        .output();
}

/// Layer 3a: Prompt endpoint returns 404 for a node that hasn't spawned yet.
#[tokio::test]
async fn prompt_endpoint_returns_404_before_spawn() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Don't create a run — just query a nonexistent one
    let resp = reqwest::get(format!(
        "{}/runs/nonexistent-run/nodes/{}/prompt?iter=1",
        daemon.url(),
        NODE_ID,
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 404);
}
