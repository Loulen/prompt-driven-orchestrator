//! Layer 3a — node_io and artifact endpoint integration tests for issue #27.
//!
//! Spawns a real TestDaemon, creates a run via POST /runs, seeds artifact files,
//! then asserts the GET /runs/{run_id}/nodes/{node_id}/io and
//! GET /runs/{run_id}/artifact endpoints return expected data.

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "io-test";
const PIPELINE_YAML: &str = r#"name: io-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: planner
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: implementer
    name: implementer
    type: code-mutating
    inputs:
      - name: plan
    outputs:
      - name: summary
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
  - source: { node: planner, port: plan }
    target: { node: implementer, port: plan }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;

    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("planner.md"), "You are a planner.\n")?;
    std::fs::write(
        prompts_dir.join("implementer.md"),
        "You are an implementer.\n",
    )?;

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
        "input": "test input for IO",
    });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

fn seed_artifacts(repo: &std::path::Path, run_id: &str) {
    let artifacts_dir = repo
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts");

    let planner_dir = artifacts_dir.join("planner/iter-1/plan");
    std::fs::create_dir_all(&planner_dir).unwrap();
    std::fs::write(planner_dir.join("output.md"), "# Plan\n\nBuild the thing.").unwrap();

    let input_dir = artifacts_dir.join("_input");
    std::fs::create_dir_all(&input_dir).unwrap();
    std::fs::write(input_dir.join("output.md"), "test input for IO").unwrap();

    let impl_dir = artifacts_dir.join("implementer/iter-1/summary");
    std::fs::create_dir_all(&impl_dir).unwrap();
    std::fs::write(
        impl_dir.join("output.md"),
        "---\nverdict: PASS\n---\n\n## Summary\nDone.",
    )
    .unwrap();
}

#[tokio::test]
async fn io_endpoint_returns_port_paths_and_frontmatter() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    seed_artifacts(daemon.repo_root(), &run_id);

    let resp = reqwest::get(format!(
        "{}/runs/{}/nodes/implementer/io?iter=1",
        daemon.url(),
        run_id,
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 200);
    let io: serde_json::Value = resp.json().await.unwrap();

    let inputs = io["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1, "implementer should have 1 input port");
    assert_eq!(inputs[0]["port"], "plan");
    assert_eq!(inputs[0]["files"][0]["exists"], true);
    assert!(
        inputs[0]["files"][0]["path"]
            .as_str()
            .unwrap()
            .contains("planner/iter-1/plan/output.md"),
        "input file path should reference planner artifact"
    );

    let outputs = io["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 1, "implementer should have 1 output port");
    assert_eq!(outputs[0]["port"], "summary");
    assert_eq!(outputs[0]["files"][0]["exists"], true);
    let fm = &outputs[0]["files"][0]["frontmatter"];
    assert_eq!(fm["verdict"], "PASS", "frontmatter should contain verdict");

    // Cleanup
    let session1 = format!("pdo-{run_id}-planner-iter-1");
    let session2 = format!("pdo-{run_id}-implementer-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session1])
        .output();
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session2])
        .output();
}

/// #370 (AC4): the `/io` endpoint — which backs the NodeRun inputs panel and the
/// `node_done` node-IO surface — must resolve a non-repeated cross-iteration edge
/// to the source's latest-COMPLETED iteration, not the consumer's own `iter`.
///
/// Scenario: the planner (a feeder) completes only at iter-1; the endpoint is
/// then asked for the implementer's inputs at iter-2 (as a loop member reading an
/// external feeder would be). The input must resolve to `planner/iter-1/...`
/// (which exists), never `planner/iter-2/...` (which never existed). Before the
/// fix the simple-wire branch walked the edge on the consumer's iter and reported
/// the input "missing".
#[tokio::test]
async fn io_endpoint_resolves_cross_iteration_input_to_latest_completed_source() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    // The planner produced its only lap at iter-1.
    let planner_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/planner/iter-1/plan");
    std::fs::create_dir_all(&planner_dir).unwrap();
    std::fs::write(
        planner_dir.join("output.md"),
        "# Plan\n\nthe feeder's only lap",
    )
    .unwrap();

    // Mark the planner COMPLETED at iter-1 so the projection records it (output
    // seeded above lets output validation pass, refs #36).
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/nodes/planner/done",
            daemon.url(),
            run_id
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "planner done should succeed: {}",
        resp.status()
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Ask for the implementer's inputs at LAP 2. The planner never ran iter-2.
    let resp = reqwest::get(format!(
        "{}/runs/{}/nodes/implementer/io?iter=2",
        daemon.url(),
        run_id,
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let io: serde_json::Value = resp.json().await.unwrap();

    let inputs = io["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1, "implementer should have 1 input port");
    assert_eq!(inputs[0]["port"], "plan");
    let path = inputs[0]["files"][0]["path"].as_str().unwrap();
    assert!(
        path.contains("planner/iter-1/plan/output.md"),
        "must resolve to the feeder's latest-completed iter-1, got: {path}"
    );
    assert!(
        !path.contains("iter-2"),
        "must NOT resolve to the consumer's positional iter-2, got: {path}"
    );
    assert_eq!(
        inputs[0]["files"][0]["exists"], true,
        "the iter-1 feeder artifact exists and must be surfaced, not reported missing"
    );

    // Cleanup
    for session in [
        format!("pdo-{run_id}-planner-iter-1"),
        format!("pdo-{run_id}-implementer-iter-1"),
    ] {
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .output();
    }
}

#[tokio::test]
async fn io_endpoint_returns_404_before_run_creation() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let resp = reqwest::get(format!(
        "{}/runs/nonexistent-run/nodes/planner/io?iter=1",
        daemon.url(),
    ))
    .await
    .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn artifact_endpoint_returns_markdown_content() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon.url()).await;

    seed_artifacts(daemon.repo_root(), &run_id);

    let resp = reqwest::get(format!(
        "{}/runs/{}/artifact?path=planner/iter-1/plan/output.md",
        daemon.url(),
        run_id,
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
    assert!(body.contains("# Plan"), "artifact should contain the plan");

    // Cleanup
    let session1 = format!("pdo-{run_id}-planner-iter-1");
    let session2 = format!("pdo-{run_id}-implementer-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session1])
        .output();
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session2])
        .output();
}
