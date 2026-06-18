//! Layer 3a — sub-worktree survival integration test for issue #32.
//!
//! Spawns a real TestDaemon with a code-mutating pipeline, creates a run,
//! marks the code-mutating node done, then asserts:
//! - the sub-worktree directory still exists on disk
//! - GET /runs/{run_id}/nodes/{node_id}/prompt?iter=1 returns 200

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "cm-survive-test";
const NODE_ID: &str = "impl-1";
const PIPELINE_YAML: &str = r#"name: cm-survive-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: impl-1
    name: impl-1
    type: code-mutating
    inputs:
      - name: task
    outputs:
      - name: summary
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: impl-1, port: task }
"#;

const ROLE_PROMPT: &str = "You are an implementer. Do the task.\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
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
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
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

/// Layer 3a: after marking a code-mutating node done, the sub-worktree
/// directory must still exist on disk and the prompt endpoint must return 200.
#[tokio::test]
async fn sub_worktree_survives_node_completion() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // The daemon creates the sub-worktree at spawn time. Verify it exists.
    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");

    assert!(
        sub_wt_dir.exists(),
        "sub-worktree should exist after run creation: {}",
        sub_wt_dir.display()
    );

    // Write a code change in the sub-worktree so merge has something to commit
    std::fs::write(sub_wt_dir.join("implementation.rs"), "fn main() {}\n").unwrap();

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1")
        .join("summary");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Summary\nDone.\n").unwrap();

    // Mark node done — triggers commit_and_merge_sub_worktree
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/nodes/{}/done",
            daemon.url(),
            run_id,
            NODE_ID,
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree directory must still exist after node completion (refs #32)
    assert!(
        sub_wt_dir.exists(),
        "sub-worktree must survive after merge for inspection (refs #32): {}",
        sub_wt_dir.display()
    );

    // Prompt endpoint must return 200 for the completed iter
    let resp = reqwest::get(format!(
        "{}/runs/{}/nodes/{}/prompt?iter=1",
        daemon.url(),
        run_id,
        NODE_ID,
    ))
    .await
    .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "prompt endpoint must return 200 for completed code-mutating node"
    );

    let body = resp.text().await.unwrap();
    assert!(!body.is_empty(), "prompt response body must be non-empty");

    // Cleanup tmux session
    let session_name = format!("pdo-{run_id}-{NODE_ID}-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session_name])
        .output();
}

/// Layer 3a: cleanup_run must still remove all sub-worktrees even though
/// they now survive merge.
#[tokio::test]
async fn cleanup_run_removes_surviving_sub_worktrees() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");

    // Write a code change and mark done
    std::fs::write(sub_wt_dir.join("implementation.rs"), "fn main() {}\n").unwrap();

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1")
        .join("summary");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Summary\nDone.\n").unwrap();

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/nodes/{}/done",
            daemon.url(),
            run_id,
            NODE_ID,
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree survives merge
    assert!(sub_wt_dir.exists());

    // Run cleanup
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree must be gone after cleanup
    assert!(
        !sub_wt_dir.exists(),
        "cleanup_run must remove sub-worktree directory"
    );

    // Run directory must be gone
    let run_dir = daemon.repo_root().join(".pdo/runs").join(&run_id);
    assert!(
        !run_dir.exists(),
        "cleanup_run must remove the run directory"
    );
}
