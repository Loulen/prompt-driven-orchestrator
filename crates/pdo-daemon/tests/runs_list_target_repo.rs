//! Layer 3a — proves issue #258: the `/runs` and `/triggers` list endpoints emit
//! a resolved `effective_repo` for the "group by project" view, while never
//! mutating the raw `target_repo`. Validates:
//! - `GET /runs` rows all carry a non-empty `effective_repo`; a run with no
//!   `target_repo` resolves to the daemon's own repo_root (no "Unassigned").
//! - `GET /triggers` rows carry a resolved `effective_repo`; the raw
//!   `target_repo` of a null-target trigger stays null (no server-side rewrite).
//! - The flattened Trigger fields (`name`, `cron`, …) stay top-level under the
//!   `effective_repo` wrapper.

mod common;

use std::process::Command;

use common::TestDaemon;

const PIPELINE_NAME: &str = "by-repo-test";
// `prompt_required: false` so a cron-only trigger with no input template is
// accepted at creation (the create handler rejects an empty resolvable input on
// a prompt-required pipeline).
const PIPELINE_YAML: &str = r#"name: by-repo-test
version: "1.0"
prompt_required: false
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

async fn create_run(daemon: &TestDaemon, body: serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed: {body:?}");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn create_trigger(daemon: &TestDaemon, body: serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "POST /triggers should succeed: {body:?}"
    );
    let json: serde_json::Value = resp.json().await.unwrap();
    json["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn runs_list_carries_resolved_effective_repo() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    // Two distinct explicit target repos + one run with no target_repo.
    let repo_a = tempfile::tempdir().unwrap();
    git_init_with_commit(repo_a.path()).unwrap();
    let repo_b = tempfile::tempdir().unwrap();
    git_init_with_commit(repo_b.path()).unwrap();
    let path_a = repo_a.path().to_str().unwrap().to_string();
    let path_b = repo_b.path().to_str().unwrap().to_string();

    create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "x", "target_repo": path_a }),
    )
    .await;
    create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "x", "target_repo": path_b }),
    )
    .await;
    // No target_repo → must resolve to the daemon's repo_root.
    create_run(
        &daemon,
        serde_json::json!({ "pipeline": PIPELINE_NAME, "input": "x" }),
    )
    .await;

    let rows: Vec<serde_json::Value> = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 3, "three runs created");

    // Every row carries a non-empty effective_repo (always concrete).
    for row in &rows {
        let er = row["effective_repo"].as_str();
        assert!(
            er.is_some() && !er.unwrap().is_empty(),
            "every run row must carry a non-empty effective_repo: {row:?}"
        );
        // No "Unassigned"/null bucket.
        assert!(!row["effective_repo"].is_null());
    }

    let repo_root = daemon.repo_root().to_str().unwrap();
    let effectives: Vec<&str> = rows
        .iter()
        .map(|r| r["effective_repo"].as_str().unwrap())
        .collect();
    assert!(effectives.contains(&path_a.as_str()));
    assert!(effectives.contains(&path_b.as_str()));
    // The null-target run resolved to the daemon's repo_root, not a separate bucket.
    assert!(
        effectives.contains(&repo_root),
        "a run with no target_repo must resolve effective_repo to the daemon repo_root \
         ({repo_root}); got {effectives:?}"
    );
}

#[tokio::test]
async fn triggers_list_resolves_effective_repo_without_mutating_raw_target() {
    let daemon = TestDaemon::spawn(seed_daemon_repo).await.unwrap();

    let repo_a = tempfile::tempdir().unwrap();
    git_init_with_commit(repo_a.path()).unwrap();
    let path_a = repo_a.path().to_str().unwrap().to_string();

    // One trigger with an explicit target, one with none.
    create_trigger(
        &daemon,
        serde_json::json!({
            "name": "t-a1",
            "pipeline_id": PIPELINE_NAME,
            "cron": "0 0 1 1 *",
            "target_repo": path_a,
        }),
    )
    .await;
    create_trigger(
        &daemon,
        serde_json::json!({
            "name": "t-null",
            "pipeline_id": PIPELINE_NAME,
            "cron": "0 0 1 1 *",
        }),
    )
    .await;

    let rows: Vec<serde_json::Value> = reqwest::get(format!("{}/triggers", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);

    let repo_root = daemon.repo_root().to_str().unwrap();
    let by_name = |name: &str| rows.iter().find(|r| r["name"] == name).unwrap();

    let t_a1 = by_name("t-a1");
    assert_eq!(t_a1["target_repo"], serde_json::json!(path_a));
    assert_eq!(t_a1["effective_repo"], serde_json::json!(path_a));

    let t_null = by_name("t-null");
    // Core no-regression guarantee: raw target_repo stays null (skip-serialized
    // ⇒ absent), but effective_repo resolved to the daemon repo_root.
    assert!(
        t_null["target_repo"].is_null(),
        "raw target_repo must NOT be rewritten server-side: {t_null:?}"
    );
    assert_eq!(t_null["effective_repo"], serde_json::json!(repo_root));

    // The #[serde(flatten)] wrapper keeps every Trigger field top-level (the
    // frontend reads name/cron/enabled/pipeline_name flat, not nested).
    for r in &rows {
        assert!(r["name"].is_string(), "name must stay top-level: {r:?}");
        assert!(r["cron"].is_string(), "cron must stay top-level: {r:?}");
        assert!(r["enabled"].is_boolean(), "enabled must stay top-level: {r:?}");
        assert!(
            r["pipeline_name"].is_string(),
            "pipeline_name must stay top-level: {r:?}"
        );
        // The trigger must not be nested under a `trigger` key.
        assert!(r.get("trigger").is_none(), "wrapper must flatten: {r:?}");
    }
}
