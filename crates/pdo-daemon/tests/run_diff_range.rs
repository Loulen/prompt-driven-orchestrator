//! Layer 3a — the aggregate Run diff endpoint over a real daemon (#376).
//!
//! Closes the AC for the backend slice (ADR-0004 golden rule: no AC closed
//! without a test at layer ≥ 3). Proves, end-to-end against a booted daemon,
//! that `GET /runs/<id>/diff`:
//!   - shows the run branch's own change,
//!   - is a **three-dot** range (main's advance *after* the fork does not
//!     surface as a phantom deletion), and
//!   - excludes the `.pdo/` blackboard.
//!
//! The entry node runs under the harness `exec sleep 600` override, so it never
//! calls `pdo complete` → the run branch is never merged and stays a clean fork
//! of the seed commit for the duration.

mod common;

use common::TestDaemon;

// --- cribbed from crates/pdo-daemon/tests/admission_concurrency.rs ---

const PIPELINE_NAME: &str = "diff-solo";
const PIPELINE_YAML: &str = r#"name: diff-solo
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: solo
    name: solo
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: solo, port: in }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
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

async fn create_run(daemon_url: String) -> Option<String> {
    let body = serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "go" });
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs"))
        .json(&body)
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json["run_id"].as_str().map(String::from)
}

#[tokio::test]
async fn run_diff_uses_three_dot_and_excludes_pdo_over_real_daemon() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.repo_root().to_path_buf();
    let run_id = create_run(daemon.url()).await.expect("run created");

    // Wait for the daemon to create the run branch + pipeline worktree
    // (<repo>/.pdo/runs/<run-id>/worktree/, see CONTEXT.md § worktree).
    let wt_dir = repo.join(".pdo/runs").join(&run_id).join("worktree");
    for _ in 0..100 {
        if wt_dir.join(".git").exists() || wt_dir.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        wt_dir.exists(),
        "pipeline worktree should exist for {run_id}"
    );

    let git = |dir: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    };
    // Real change on the run branch (via the daemon-created pipeline worktree).
    std::fs::write(wt_dir.join("feature.rs"), "fn feature() {}\n").unwrap();
    std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
    std::fs::write(wt_dir.join(".pdo/artifact.txt"), "blackboard\n").unwrap();
    git(&wt_dir, &["add", "feature.rs"]);
    git(&wt_dir, &["add", "-f", ".pdo/artifact.txt"]);
    git(&wt_dir, &["commit", "-m", "run work + artifact"]);

    // Advance main after the fork.
    std::fs::write(repo.join("unrelated.rs"), "fn unrelated() {}\n").unwrap();
    git(&repo, &["add", "unrelated.rs"]);
    git(&repo, &["commit", "-m", "advance main"]);

    let body = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/diff", daemon.url()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("feature.rs") && body.contains("fn feature()"),
        "real run change present: {body}"
    );
    assert!(
        !body.contains("unrelated.rs"),
        "no phantom deletion of main's advance: {body}"
    );
    assert!(
        !body.contains("artifact.txt"),
        "blackboard excluded: {body}"
    );
}
