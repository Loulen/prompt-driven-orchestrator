//! Layer 3a — Switch `when:` clause validation against upstream schema (issue #64).
//!
//! Spawns a real TestDaemon and tests:
//!   1. PUT /pipelines with a Switch `when:` referencing an undeclared field → 400
//!   2. PUT /pipelines with a Switch `when:` referencing a declared field → 200

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "switch-val";

const VALID_PIPELINE: &str = r#"name: switch-val
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { eq: PASS }
      - name: default
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
  - source: { node: gate, port: pass }
    target: { node: end, port: result }
"#;

const INVALID_PIPELINE: &str = r#"name: switch-val
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          nonexistent: { eq: PASS }
      - name: default
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
  - source: { node: gate, port: pass }
    target: { node: end, port: result }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".maestro").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        VALID_PIPELINE,
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
    run(&["init"])?;
    run(&["config", "user.email", "test@test.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["add", "."])?;
    run(&["commit", "-m", "init"])?;
    Ok(())
}

#[tokio::test]
async fn save_rejects_undeclared_when_field() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{}/pipelines/{PIPELINE_NAME}", daemon.url()))
        .json(&serde_json::json!({
            "yaml": INVALID_PIPELINE,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    let err = body["error"].as_str().unwrap();
    assert!(
        err.contains("nonexistent") && err.contains("not found in upstream schema"),
        "error should describe the undeclared field: {err}"
    );
}

#[tokio::test]
async fn save_accepts_valid_typed_when_field() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{}/pipelines/{PIPELINE_NAME}", daemon.url()))
        .json(&serde_json::json!({
            "yaml": VALID_PIPELINE,
            "prompts": {}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}
