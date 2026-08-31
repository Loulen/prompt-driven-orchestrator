//! Layer 3 (#552, ADR-0046) — the **Projet** tier of the harness axis, end to end
//! through the daemon.
//!
//! A single unpinned `doc-only` node, spawned through the **tmux command seam** (a
//! harmless `sleep`, never a real agent). With no node pin and no instance
//! default, the only tier that can name a harness is the **Projet** of the Run's
//! primary repo. The tests prove:
//!   1. a Projet posed on the primary repo resolves the node's harness (and is
//!      **frozen** on `NodeStarted`), while a Projet on a *secondary* repo is
//!      ignored — the Run follows its **primary** (ADR-0042);
//!   2. with no Projet, resolution falls through to the `claude` floor; and
//!   3. attaching a path already owned by another Projet is refused, **naming**
//!      the owner (AC), before any effect.
//!
//! Nothing is seeded: every Projet here is materialised via `POST /projects`.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "proj-harness-test";
const PIPELINE_YAML: &str = r#"name: proj-harness-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aaaaaaaa
    name: worker
    type: doc-only
    outputs:
      - name: out
    view: { x: 200, y: 60 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aaaaaaaa, port: task }
  - source: { node: aaaaaaaa, port: out }
    target: { node: end, port: result }
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
    std::fs::write(prompts_dir.join("aaaaaaaa.md"), "You are the worker.\n")?;
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

/// A fresh git repo (one commit) at `dir`, ready to be pinned as a secondary.
fn make_secondary_repo(dir: &std::path::Path) {
    std::fs::write(dir.join("README.md"), "secondary\n").unwrap();
    git_init_with_commit(dir).unwrap();
}

async fn create_project(daemon: &TestDaemon, name: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/projects", daemon.url()))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /projects should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["id"].as_str().unwrap().to_string()
}

async fn add_member(daemon: &TestDaemon, project_id: &str, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/projects/{project_id}/members", daemon.url()))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap()
}

async fn set_harness(daemon: &TestDaemon, project_id: &str, harness: &str) {
    let resp = reqwest::Client::new()
        .patch(format!("{}/projects/{project_id}", daemon.url()))
        .json(&serde_json::json!({ "harness": harness }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PATCH /projects should return 200");
}

async fn create_run(daemon: &TestDaemon, body: serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should return 201");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn events(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let v: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v.as_array().cloned().unwrap_or_default()
}

/// The `harness` frozen on the worker node's latest `NodeStarted`, or `None`.
fn started_harness(evs: &[serde_json::Value]) -> Option<String> {
    evs.iter()
        .rev()
        .find(|e| e["kind"] == "node_started" && e["node_id"] == "aaaaaaaa")
        .and_then(|e| e["payload"]["harness"].as_str())
        .map(String::from)
}

async fn wait_for_started(daemon: &TestDaemon, run_id: &str) -> String {
    for _ in 0..50 {
        let evs = events(daemon, run_id).await;
        if let Some(h) = started_harness(&evs) {
            return h;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("worker node should have started within the timeout");
}

#[tokio::test]
async fn projet_of_primary_repo_resolves_the_node_harness_secondary_ignored() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let primary = daemon.target_repo();

    // A secondary git repo, owned by a DIFFERENT Projet carrying `claude`. If the
    // resolver ever consulted a secondary, this would drag the node onto claude.
    let secondary = tempfile::tempdir().unwrap();
    make_secondary_repo(secondary.path());
    let secondary_path = secondary.path().to_str().unwrap().to_string();

    // Materialise the Projets via the API (nothing is seeded) and pose harnesses.
    let product = create_project(&daemon, "Product").await;
    assert_eq!(
        add_member(&daemon, &product, &primary).await.status(),
        200,
        "primary attaches cleanly"
    );
    set_harness(&daemon, &product, "opencode").await;

    let legacy = create_project(&daemon, "Legacy").await;
    assert_eq!(
        add_member(&daemon, &legacy, &secondary_path).await.status(),
        200
    );
    set_harness(&daemon, &legacy, "claude").await;

    // A Run on the primary, carrying the secondary read-only. The unpinned node
    // has no node/Run/instance harness — only the Projet tier can name one.
    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": primary,
            "target_repos": [ { "repo": primary }, { "repo": secondary_path } ],
        }),
    )
    .await;

    // The node froze `opencode` — the primary's Projet — NOT `claude` from the
    // secondary's Projet, and NOT the bare `claude` floor.
    assert_eq!(
        wait_for_started(&daemon, &run_id).await,
        "opencode",
        "the Run follows its primary repo's Projet; the secondary's Projet is ignored"
    );
}

#[tokio::test]
async fn no_projet_falls_through_to_the_claude_floor() {
    // Control: with no Projet naming a harness, an unpinned node resolves the
    // `claude` floor — the pre-#552 behaviour, unchanged.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(wait_for_started(&daemon, &run_id).await, "claude");
}

#[tokio::test]
async fn attaching_a_path_to_a_second_projet_is_refused_naming_the_owner() {
    // AC end to end: a path already owned by a Projet cannot join a second — the
    // refusal is a 409 that NAMES the owner, before any effect.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let primary = daemon.target_repo();

    let alpha = create_project(&daemon, "Alpha").await;
    assert_eq!(add_member(&daemon, &alpha, &primary).await.status(), 200);

    let bravo = create_project(&daemon, "Bravo").await;
    let refused = add_member(&daemon, &bravo, &primary).await;
    assert_eq!(refused.status(), 409, "a second attach is refused");
    let body: serde_json::Value = refused.json().await.unwrap();
    assert_eq!(body["owner_name"].as_str(), Some("Alpha"));
    assert!(
        body["error"].as_str().unwrap_or_default().contains("Alpha"),
        "the refusal message names the owning Projet: {body}"
    );

    // Before any effect: Bravo gained no member.
    let bravo_state: serde_json::Value = reqwest::get(format!("{}/projects/{bravo}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        bravo_state["members"].as_array().unwrap().is_empty(),
        "the refused Projet must gain no member"
    );
}
