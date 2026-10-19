//! Layer 3 (#552, ADR-0046) — the **Projet** tier of the harness axis, end to end
//! through the daemon.
//!
//! A single unpinned non-isolated node, spawned through the **tmux command seam** (a
//! harmless `sleep`, never a real agent). With no node pin and no instance
//! default, the only tier that can name a harness is the **Projet** the Run was
//! launched in (ADR-0087). The tests prove:
//!   1. the Projet a Run names resolves the node's harness (and is **frozen** on
//!      `NodeStarted`), whatever the other Projets holding its repositories;
//!   2. with no Projet named, resolution falls through to the `claude` floor,
//!      even when the repository is a member of a Projet; and
//!   3. a path may join a second Projet, each Run following the one it names.
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
    type: agent
    isolated_worktree: false
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
async fn the_projet_a_run_names_resolves_the_node_harness() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let primary = daemon.target_repo();

    // A secondary git repo, member of a DIFFERENT Projet carrying `claude`. If the
    // resolver ever consulted a repository's Projets, this would drag the node
    // onto claude.
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
    assert_eq!(add_member(&daemon, &legacy, &primary).await.status(), 200);
    set_harness(&daemon, &legacy, "claude").await;

    // A Run launched in Product, carrying the secondary read-only. The unpinned
    // node has no node/Run/instance harness — only the Projet tier can name one.
    let run_id = create_run(
        &daemon,
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": primary,
            "project_id": product,
            "target_repos": [ { "repo": primary }, { "repo": secondary_path } ],
        }),
    )
    .await;

    // The node froze `opencode` — the Run's Projet — NOT `claude` from the
    // other Projet holding its repositories, and NOT the bare `claude` floor.
    assert_eq!(
        wait_for_started(&daemon, &run_id).await,
        "opencode",
        "the Run follows the Projet it was launched in"
    );
}

#[tokio::test]
async fn no_projet_falls_through_to_the_claude_floor() {
    // Control: with no Projet named, an unpinned node resolves the `claude`
    // floor — even on a repository a Projet carrying `opencode` holds
    // (ADR-0087: the Projet is chosen at launch, never deduced).
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let product = create_project(&daemon, "Product").await;
    assert_eq!(
        add_member(&daemon, &product, &daemon.target_repo())
            .await
            .status(),
        200
    );
    set_harness(&daemon, &product, "opencode").await;
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
async fn a_path_may_join_a_second_projet() {
    // ADR-0087: no more « already in another Projet ».
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let primary = daemon.target_repo();

    let alpha = create_project(&daemon, "Alpha").await;
    assert_eq!(add_member(&daemon, &alpha, &primary).await.status(), 200);
    let bravo = create_project(&daemon, "Bravo").await;
    assert_eq!(add_member(&daemon, &bravo, &primary).await.status(), 200);

    for project in [&alpha, &bravo] {
        let state: serde_json::Value = reqwest::get(format!("{}/projects/{project}", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(state["members"], serde_json::json!([primary]));
        assert_eq!(state["primary_repo"], serde_json::json!(primary));
    }
}

/// `GET /runs`, as JSON.
async fn runs_list(daemon: &TestDaemon) -> Vec<serde_json::Value> {
    reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn one_repository_in_two_projets_each_run_recorded_in_the_projet_it_names() {
    // CORE-95's Feature Path, through the API: the same repository in P1 and
    // P2, a Run launched in each, one in none, and a Trigger firing in P2.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let repo = daemon.target_repo();
    let p1 = create_project(&daemon, "P1").await;
    let p2 = create_project(&daemon, "P2").await;
    for project in [&p1, &p2] {
        assert_eq!(add_member(&daemon, project, &repo).await.status(), 200);
    }
    let body = |project: Option<&str>| {
        serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": repo,
            "project_id": project,
        })
    };
    let in_p1 = create_run(&daemon, body(Some(&p1))).await;
    let in_p2 = create_run(&daemon, body(Some(&p2))).await;
    let in_none = create_run(&daemon, body(None)).await;

    let list = runs_list(&daemon).await;
    let project_of = |run: &str| {
        list.iter()
            .find(|r| r["run_id"] == run)
            .map(|r| r["project_id"].clone())
            .unwrap()
    };
    assert_eq!(project_of(&in_p1), serde_json::json!(p1));
    assert_eq!(project_of(&in_p2), serde_json::json!(p2));
    assert_eq!(project_of(&in_none), serde_json::Value::Null);
    let detail: serde_json::Value = reqwest::get(format!("{}/runs/{in_p2}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["project_id"], serde_json::json!(p2), "{detail}");

    // A named Projet must exist and hold the primary repository.
    let refused = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body(Some("prj-nope")))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400);
    let elsewhere = tempfile::tempdir().unwrap();
    make_secondary_repo(elsewhere.path());
    let refused = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": PIPELINE_NAME,
            "input": "go",
            "target_repo": elsewhere.path(),
            "project_id": p1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400);
    let refusal: serde_json::Value = refused.json().await.unwrap();
    assert!(
        refusal["error"]
            .as_str()
            .unwrap()
            .contains("is not a member of project `P1`"),
        "{refusal}"
    );

    // A Trigger carries its Projet like a Run, and fires in it.
    let trigger: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .json(&serde_json::json!({
            "name": "nightly",
            "pipeline_id": PIPELINE_NAME,
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": repo,
            "project_id": p2,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(trigger["project_id"], serde_json::json!(p2), "{trigger}");
    let fired: serde_json::Value = reqwest::Client::new()
        .post(format!(
            "{}/triggers/{}/fire",
            daemon.url(),
            trigger["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let fired_run = fired["run_id"].as_str().expect("the fire starts a Run");
    let list = runs_list(&daemon).await;
    let fired_project = list
        .iter()
        .find(|r| r["run_id"] == fired_run)
        .map(|r| r["project_id"].clone());
    assert_eq!(fired_project, Some(serde_json::json!(p2)));
}
