//! Layer 3a — mechanical parent provenance + children endpoint (issue #720,
//! ADR-0064). Boots a real daemon and probes its HTTP surface:
//!
//! - A run created via `POST /runs` **from a live node session** (the
//!   `X-PDO-Session-Run-Id` / `X-PDO-Session-Node-Id` headers, same values as
//!   the `PDO_RUN_ID` / `PDO_NODE_ID` env vars a node is wrapped with) carries
//!   `parent_run_id` + `parent_node_id` on `GET /runs/{id}` and `GET /runs` —
//!   and its project defaults to the parent's (ADR-0033: still a required
//!   boundary, so the default is the PARENT's repo, never the daemon root).
//! - A body declaring a parent (`parent_run_id` / `parent_node_id`) is refused
//!   with a named `400` — provenance is never declarable.
//! - A session claim that names no live node session is refused `403` before
//!   any effect; a create without a claim is a root run (no parent fields).
//! - `GET /runs/{id}/children` lists children grouped by parent node with
//!   status, `started_at`, `completed_at` and a read-derived cost (absent here,
//!   rendered "—" — never `$0`, ADR-0052); a childless run returns an empty
//!   list and an unknown run a `404`.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "provenance-test";
const PIPELINE_YAML: &str = r#"name: provenance-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
"#;

const CHILD_PIPELINE_NAME: &str = "provenance-child";
const CHILD_PIPELINE_YAML: &str = r#"name: provenance-child
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

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

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are a worker.\n")?;
    std::fs::write(
        pipelines_dir.join(format!("{CHILD_PIPELINE_NAME}.yaml")),
        CHILD_PIPELINE_YAML,
    )?;
    git_init_with_commit(repo)?;
    Ok(())
}

/// The distinct repo every create names explicitly (the boundary ADR-0033 keeps).
fn other_repo() -> anyhow::Result<String> {
    let repo = tempfile::tempdir()?;
    git_init_with_commit(repo.path())?;
    let path = repo.path().to_string_lossy().to_string();
    // Leak the tempdir: the daemon reads the repo later (worktree creation).
    std::mem::forget(repo);
    Ok(path)
}

async fn create_root_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "hello world",
        "target_repo": daemon.target_repo(),
    });
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

async fn get_json(daemon: &TestDaemon, path: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::get(format!("{}{}", daemon.url(), path))
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().await.unwrap();
    (status, json)
}

/// Post a create-run body with the given session headers.
async fn post_run(
    daemon: &TestDaemon,
    body: serde_json::Value,
    session: Option<(&str, &str)>,
) -> reqwest::Response {
    let mut req = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body);
    if let Some((run_id, node_id)) = session {
        req = req
            .header("X-PDO-Session-Run-Id", run_id)
            .header("X-PDO-Session-Node-Id", node_id);
    }
    req.send().await.unwrap()
}

/// Wait until the node projects as Running (its tmux session holds).
async fn wait_node_running(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    for _ in 0..100 {
        let (status, run) = get_json(daemon, &format!("/runs/{run_id}")).await;
        assert_eq!(status, 200);
        if run["nodes"][node_id]["status"] == "running" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("node {node_id} of run {run_id} never reached `running`");
}

/// Wait for the parent's `worker` node to hold a live session. The scheduler
/// spawns it once the start node completes; the TestDaemon's tmux override
/// (`exec sleep 600`) keeps the session alive indefinitely.
async fn start_worker(daemon: &TestDaemon, run_id: &str) {
    wait_node_running(daemon, run_id, "worker").await;
}

#[tokio::test]
async fn child_run_from_node_session_carries_mechanical_parent() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_root_run(&daemon).await;
    start_worker(&daemon, &parent).await;

    // Created FROM the worker's session, with NO target_repo: the child's
    // project defaults to the parent's (spec #719: project du parent par
    // défaut), never to the daemon root (ADR-0033 keeps the boundary).
    let resp = post_run(
        &daemon,
        serde_json::json!({
            "pipeline": CHILD_PIPELINE_NAME,
            "input": "child work",
        }),
        Some((&parent, "worker")),
    )
    .await;
    assert_eq!(resp.status(), 201, "session-backed create should succeed");
    let child: serde_json::Value = resp.json().await.unwrap();
    let child_id = child["run_id"].as_str().unwrap().to_string();

    // GET /runs/{id} exposes the mechanical provenance.
    let (status, run) = get_json(&daemon, &format!("/runs/{child_id}")).await;
    assert_eq!(status, 200);
    assert_eq!(
        run["parent_run_id"], parent,
        "parent_run_id is the session's run"
    );
    assert_eq!(
        run["parent_node_id"], "worker",
        "parent_node_id is the session's node"
    );
    assert_eq!(
        run["target_repo"],
        daemon.target_repo(),
        "child project defaults to the parent's"
    );

    // GET /runs exposes the same fields for the badge and the root-only filter.
    let (status, runs) = get_json(&daemon, "/runs").await;
    assert_eq!(status, 200);
    let entry = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == child["run_id"])
        .expect("child run in list");
    assert_eq!(entry["parent_run_id"], parent);
    assert_eq!(entry["parent_node_id"], "worker");

    // The root run itself carries no parent (root run).
    let root_entry = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == parent)
        .expect("root run in list");
    assert!(
        root_entry.get("parent_run_id").is_none(),
        "a root run has no parent_run_id"
    );
    assert!(root_entry.get("parent_node_id").is_none());
}

#[tokio::test]
async fn declared_parent_is_refused_out_of_session() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    // Provenance never travels in the body: the two keys are not accepted fields.
    for field in ["parent_run_id", "parent_node_id"] {
        let body = serde_json::json!({
            "pipeline": CHILD_PIPELINE_NAME,
            "input": "child work",
            "target_repo": daemon.target_repo(),
            field: "some-run",
        });
        let resp = post_run(&daemon, body, None).await;
        assert_eq!(resp.status(), 400, "declared `{field}` must be refused");
        let err: serde_json::Value = resp.json().await.unwrap();
        let msg = err["error"].as_str().unwrap_or("");
        assert!(
            msg.contains(field),
            "refusal names the offending field `{field}`: {msg}"
        );
    }

    // …and even FROM a session, a declared parent stays refused: the daemon
    // decides the parent, the body never does.
    let parent = create_root_run(&daemon).await;
    start_worker(&daemon, &parent).await;
    let body = serde_json::json!({
        "pipeline": CHILD_PIPELINE_NAME,
        "input": "child work",
        "parent_run_id": "run-i-declare",
    });
    let resp = post_run(&daemon, body, Some((&parent, "worker"))).await;
    assert_eq!(
        resp.status(),
        400,
        "a declared parent is refused even in session"
    );
}

#[tokio::test]
async fn bogus_or_stale_session_claim_is_refused() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let body = serde_json::json!({
        "pipeline": CHILD_PIPELINE_NAME,
        "input": "child work",
        "target_repo": daemon.target_repo(),
    });

    // Unknown run.
    let resp = post_run(&daemon, body.clone(), Some(("no-such-run", "worker"))).await;
    assert_eq!(resp.status(), 403, "a claim on an unknown run is refused");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert!(
        err["error"].as_str().unwrap_or("").contains("no-such-run"),
        "refusal names the claimed run: {:?}",
        err
    );

    // Real run, unknown node.
    let parent = create_root_run(&daemon).await;
    let resp = post_run(&daemon, body.clone(), Some((&parent, "no-such-node"))).await;
    assert_eq!(resp.status(), 403, "a claim on an unknown node is refused");

    // A node whose session is GONE: complete the worker explicitly (`pdo
    // complete`'s endpoint), which detaches its session and leaves the node
    // `completed` — holding no session.
    let exited = TestDaemon::spawn_with_override(seed, Some("exec true".to_string()))
        .await
        .unwrap();
    let done_parent = create_root_run(&exited).await;
    wait_node_running(&exited, &done_parent, "worker").await;
    // The completion validator requires the node's declared output; write it
    // into the run worktree like the (already-dead) session would have.
    let out_dir = exited
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(&done_parent)
        .join("worktree")
        .join(".pdo")
        .join("artifacts")
        .join("worker")
        .join("iter-1")
        .join("result");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("output.md"), "done\n").unwrap();
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{done_parent}/nodes/worker/done",
            exited.url()
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "worker completion should succeed");
    for _ in 0..100 {
        let (status, run) = get_json(&exited, &format!("/runs/{done_parent}")).await;
        assert_eq!(status, 200);
        if run["nodes"]["worker"]["status"] == "completed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let done_body = serde_json::json!({
        "pipeline": CHILD_PIPELINE_NAME,
        "input": "child work",
        "target_repo": exited.target_repo(),
    });
    let resp = post_run(&exited, done_body, Some((&done_parent, "worker"))).await;
    assert_eq!(resp.status(), 403, "a node holding no session is refused");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert!(
        err["error"]
            .as_str()
            .unwrap_or("")
            .contains("holds no live session"),
        "refusal says why: {:?}",
        err
    );

    // No run was created by any of the refused claims.
    let (status, runs) = get_json(&daemon, "/runs").await;
    assert_eq!(status, 200);
    assert_eq!(
        runs.as_array().unwrap().len(),
        1,
        "refused claims create nothing"
    );
}

#[tokio::test]
async fn run_created_without_session_is_a_root() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_root_run(&daemon).await;

    let (status, run) = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(status, 200);
    assert!(
        run.get("parent_run_id").is_none(),
        "an out-of-session create is a root run"
    );
    assert!(run.get("parent_node_id").is_none());

    // An explicit target repo still wins over the (absent) parent default.
    let explicit = other_repo().unwrap();
    let resp = post_run(
        &daemon,
        serde_json::json!({
            "pipeline": CHILD_PIPELINE_NAME,
            "input": "sibling work",
            "target_repo": explicit,
        }),
        None,
    )
    .await;
    assert_eq!(resp.status(), 201);
    let sibling: serde_json::Value = resp.json().await.unwrap();
    let (status, run) = get_json(
        &daemon,
        &format!("/runs/{}", sibling["run_id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(run["target_repo"], explicit, "explicit repo override holds");
}

#[tokio::test]
async fn children_endpoint_lists_children_grouped_by_parent_node() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_root_run(&daemon).await;
    start_worker(&daemon, &parent).await;

    // Two children from the same node session, one explicitly re-pointed at a
    // different repo (overridable, spec #719 story 13) — still the parent's
    // children: no special inheritance, an ordinary run. Names are explicit so
    // the assertions don't race the manager's async auto-naming.
    let explicit_repo = other_repo().unwrap();
    for (name, repo) in [
        ("child one", None),
        ("child two", Some(explicit_repo.as_str())),
    ] {
        let mut body = serde_json::json!({
            "pipeline": CHILD_PIPELINE_NAME,
            "input": name,
            "name": name,
        });
        if let Some(repo) = repo {
            body["target_repo"] = serde_json::json!(repo);
        }
        let resp = post_run(&daemon, body, Some((&parent, "worker"))).await;
        assert_eq!(resp.status(), 201);
    }

    let (status, children) = get_json(&daemon, &format!("/runs/{parent}/children")).await;
    assert_eq!(status, 200);
    assert_eq!(children["run_id"], parent);

    let nodes = children["nodes"].as_array().expect("nodes is an array");
    assert_eq!(nodes.len(), 1, "grouped by the ONE parent node used");
    assert_eq!(nodes[0]["node_id"], "worker");

    let group = nodes[0]["children"]
        .as_array()
        .expect("children is an array");
    assert_eq!(group.len(), 2, "both children listed");

    // Chronological within the group: "child one" first.
    assert_eq!(group[0]["name"], "child one");
    assert_eq!(group[1]["name"], "child two");

    for child in group {
        assert!(child["run_id"].is_string());
        assert_eq!(child["pipeline_name"], CHILD_PIPELINE_NAME);
        assert_eq!(
            child["status"], "running",
            "a child is an ordinary live run"
        );
        // #783 — `stalled` is derived on read like on `GET /runs` (#180), so the
        // Orchestration tab's stale pill has a source. A fresh live child is not
        // stalled; the field is always present (never omitted like `cost`).
        assert_eq!(
            child["stalled"], false,
            "stalled present and false on a fresh live child: {:?}",
            child
        );
        assert!(
            child["started_at"].is_string(),
            "started_at present: {:?}",
            child
        );
        assert!(
            child.get("completed_at").is_none(),
            "a live child has no completed_at"
        );
        // ADR-0052: the cost is derived on read; with no transcripts to read it
        // is ABSENT, and the surface renders "—" — never `$0`.
        assert!(
            child.get("cost").is_none() || child["cost"].is_null(),
            "unavailable cost is never fabricated: {:?}",
            child
        );
    }

    // An explicit repo override did not change the parent link (mechanical).
    assert!(group[1]["run_id"].is_string());

    // A child is itself listed as a parent of nothing: its children list is empty.
    let child_id = group[0]["run_id"].as_str().unwrap().to_string();
    let (status, grandchildren) = get_json(&daemon, &format!("/runs/{child_id}/children")).await;
    assert_eq!(status, 200);
    assert_eq!(
        grandchildren["nodes"].as_array().unwrap().len(),
        0,
        "a childless run returns an empty list"
    );
}

#[tokio::test]
async fn children_endpoint_404_for_unknown_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::get(format!("{}/runs/no-such-run/children", daemon.url()))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        404,
        "an unknown run is a 404, not an empty list"
    );
}
