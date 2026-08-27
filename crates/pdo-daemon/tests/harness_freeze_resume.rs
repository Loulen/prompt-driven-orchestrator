//! Layer 3a (#550, ADR-0046) — the harness axis, end to end through the daemon.
//!
//! A two-node `doc-only` pipeline: one node with no pin (⇒ the `claude` floor),
//! one `pin_harness: opencode`. Spawned through the **tmux command seam** (a
//! harmless `sleep`, never a real agent), the test proves:
//!   1. the resolved harness is **frozen** in each node's `NodeStarted` event, and
//!   2. a resume of the pinned node **re-poses** that frozen harness (ADR-0007).
//!
//! The binary fail-fast (AC #10) is deliberately not exercised here: it is
//! skipped under the command override (an override means the real binary never
//! runs), and it is unit-tested in `node_spawn` / `node_primitives`.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "harness-test";
const PIPELINE_YAML: &str = r#"name: harness-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: aaaaaaaa
    name: on-claude
    type: doc-only
    outputs:
      - name: out
    view: { x: 200, y: 60 }
  - id: bbbbbbbb
    name: on-opencode
    type: doc-only
    pin_harness: opencode
    outputs:
      - name: out
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aaaaaaaa, port: task }
  - source: { node: start, port: user_prompt }
    target: { node: bbbbbbbb, port: task }
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
    std::fs::write(prompts_dir.join("aaaaaaaa.md"), "You are on claude.\n")?;
    std::fs::write(prompts_dir.join("bbbbbbbb.md"), "You are on opencode.\n")?;
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

async fn create_run(daemon: &TestDaemon) -> String {
    create_run_with_harness(daemon, None).await
}

/// Create a Run, optionally choosing a **Run-level harness** (#551) — the `run` tier.
async fn create_run_with_harness(daemon: &TestDaemon, harness: Option<&str>) -> String {
    let mut body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "go",
        "target_repo": daemon.target_repo(),
    });
    if let Some(h) = harness {
        body["harness"] = serde_json::json!(h);
    }
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

/// The Run's projected state (`GET /runs/<id>`), which serializes `RunState` — so its
/// `harness` key is the frozen Run harness the panel shows (#551).
async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
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

/// The `harness` frozen on a node's latest `NodeStarted`, or `None` until it lands.
fn started_harness(evs: &[serde_json::Value], node_id: &str) -> Option<String> {
    evs.iter()
        .rev()
        .find(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
        .and_then(|e| e["payload"]["harness"].as_str())
        .map(String::from)
}

/// Poll the event log until both entry nodes have a `NodeStarted`, or time out.
async fn wait_for_both_started(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        let evs = events(daemon, run_id).await;
        if started_harness(&evs, "aaaaaaaa").is_some()
            && started_harness(&evs, "bbbbbbbb").is_some()
        {
            return evs;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("both entry nodes should have started within the timeout");
}

#[tokio::test]
async fn resolved_harness_is_frozen_on_node_started() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    let evs = wait_for_both_started(&daemon, &run_id).await;

    // The unpinned node froze the `claude` floor; the pinned node froze `opencode`
    // — the resolved harness, not what any tier says now (ADR-0046, gel au spawn).
    assert_eq!(started_harness(&evs, "aaaaaaaa").as_deref(), Some("claude"));
    assert_eq!(
        started_harness(&evs, "bbbbbbbb").as_deref(),
        Some("opencode")
    );
}

#[tokio::test]
async fn stats_exposes_frozen_harnesses_through_the_real_daemon() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_both_started(&daemon, &run_id).await;
    let period = "from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=day";

    let overview: serde_json::Value =
        reqwest::get(format!("{}/stats/overview?{period}", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(
        overview["session_harnesses"],
        serde_json::json!(["claude", "opencode"])
    );
    let pipeline = overview["sessions_by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == PIPELINE_NAME)
        .expect("the started Run belongs to the harness test Pipeline");
    assert_eq!(pipeline["executions"], 2);
    assert_eq!(
        pipeline["harnesses"],
        serde_json::json!([
            {"harness":"claude","executions":1},
            {"harness":"opencode","executions":1}
        ])
    );

    let cost: serde_json::Value =
        reqwest::get(format!("{}/stats/cost?{period}", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(cost["harnesses"], serde_json::json!(["claude", "opencode"]));
    let pipeline = cost["by_pipeline"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == PIPELINE_NAME)
        .expect("cost hierarchy keeps the Run's Pipeline");
    assert_eq!(pipeline["executions"], 1);
    let opencode = pipeline["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["harness"] == "opencode")
        .expect("a harness stays visible when it has no cost source");
    assert!(opencode["usd"].is_null());
    assert_eq!(opencode["unknown"], 1);
    assert_eq!(
        opencode["missing_reasons"],
        serde_json::json!(["harness has no cost source"])
    );
}

#[tokio::test]
async fn resume_reposes_the_frozen_harness() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_both_started(&daemon, &run_id).await;

    // Kill the pinned node's live session out-of-band (on the daemon's own tmux
    // socket) so the pane endpoint has to RESUME rather than merely capture.
    let socket = daemon.tmux_socket();
    let session = format!("pdo-{run_id}-bbbbbbbb-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", &session])
        .output();

    // The pane endpoint reads the frozen `opencode` harness back from `NodeStarted`,
    // finds it can resume, and re-poses it (via the command seam). It must not
    // answer a bare "session no longer available".
    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/bbbbbbbb/pane",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let pane: serde_json::Value = resp.json().await.unwrap();
    assert_ne!(
        pane["source"].as_str(),
        Some("unavailable"),
        "a resumable pinned node must re-pose, not report unavailable: {pane}"
    );
}

// -----------------------------------------------------------------------------
// #551 — the Run tier (`nœud → Run → instance → plancher`), end to end.
// -----------------------------------------------------------------------------

/// Poll for the manager session (`pdo-mgr-<run>`) to come up on the daemon's own tmux
/// socket, proving the manager spawned on the resolved harness without failing fast.
async fn wait_for_manager_session(daemon: &TestDaemon, run_id: &str) -> bool {
    let socket = daemon.tmux_socket();
    let session = format!("pdo-mgr-{run_id}");
    for _ in 0..50 {
        let up = std::process::Command::new("tmux")
            .args(["-L", &socket, "has-session", "-t", &session])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if up {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

/// The Run tier moves a **free** node off the floor: a Run created on `opencode` freezes
/// `opencode` into the unpinned node's `NodeStarted` (it followed the Run), while the
/// Run's own frozen harness is visible in `GET /runs/<id>` (the Run panel, AC), and the
/// Pipeline Manager comes up on that harness (the infra session follows the Run — AC).
#[tokio::test]
async fn run_harness_moves_the_free_node_and_manager() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run_with_harness(&daemon, Some("opencode")).await;
    let evs = wait_for_both_started(&daemon, &run_id).await;

    // The unpinned node followed the Run tier off the `claude` floor to `opencode`…
    assert_eq!(
        started_harness(&evs, "aaaaaaaa").as_deref(),
        Some("opencode"),
        "a free node must follow the Run's harness"
    );
    // …and the pinned node stays `opencode` too (its pin agrees with the Run here).
    assert_eq!(
        started_harness(&evs, "bbbbbbbb").as_deref(),
        Some("opencode")
    );

    // The frozen Run harness is visible in the Run panel (AC): `GET /runs/<id>`.
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(
        run["harness"].as_str(),
        Some("opencode"),
        "the Run panel must show the frozen Run harness: {run}"
    );

    // The Pipeline Manager followed the Run's harness (AC): its session came up, so the
    // manager spawn resolved `opencode` and did not fail fast. (The exact harness the
    // manager launches is proven purely by `harness_resolver::resolve_infra_harness`;
    // the tmux command override erases the tail, so an L3 asserts the spawn, not bytes.)
    assert!(
        wait_for_manager_session(&daemon, &run_id).await,
        "the manager session must come up on the Run's harness"
    );
}

/// A **pinned** node resists the Run tier: a Run created on `claude` cannot pull the
/// `opencode`-pinned node off its pin (ADR-0046, épinglage ≠ paramétrage), while the free
/// node follows the Run down to `claude`.
#[tokio::test]
async fn pinned_node_resists_the_run_harness() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run_with_harness(&daemon, Some("claude")).await;
    let evs = wait_for_both_started(&daemon, &run_id).await;

    // The pinned node ignores the Run's `claude` choice and stays on its `opencode` pin.
    assert_eq!(
        started_harness(&evs, "bbbbbbbb").as_deref(),
        Some("opencode"),
        "a pinned node must resist the Run harness"
    );
    // The free node follows the Run to `claude`.
    assert_eq!(started_harness(&evs, "aaaaaaaa").as_deref(), Some("claude"));

    // And the Run panel shows the Run's own frozen choice.
    let run = get_run(&daemon, &run_id).await;
    assert_eq!(run["harness"].as_str(), Some("claude"), "{run}");
}

/// A Run created with **no** harness names none: the free node stays on the floor and the
/// Run panel omits the key (byte-identical to the pre-#551 shape).
#[tokio::test]
async fn a_run_without_a_harness_freezes_none() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_both_started(&daemon, &run_id).await;

    let run = get_run(&daemon, &run_id).await;
    assert!(
        run.get("harness").is_none() || run["harness"].is_null(),
        "a Run that named no harness must not carry the key: {run}"
    );
}
