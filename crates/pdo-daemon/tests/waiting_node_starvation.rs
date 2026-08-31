//! Layer 3a — #509: a node throttled into `waiting` by the admission cap is
//! re-driven when a slot frees through a path that does **not** already re-drive
//! the queue.
//!
//! `retry_waiting_nodes` has no timer of its own — every one of its callers is
//! event-driven (#159) — so a slot-freeing path that forgets to call it strands
//! the queued node for ever. #489-C closed the `kill_node` path; this file pins
//! the two that stayed open:
//!
//! 1. **Boot recovery.** Failing an orphaned `Running` node at daemon startup
//!    frees its slot, but a Run whose orphan died while a *sibling* node is still
//!    `Running` never reaches the run-level stall reconciliation (a live sibling
//!    suppresses `run_stall_reason`), so nothing redistributed the slot. This is
//!    the path reproduced empirically on the issue.
//! 2. **A command that drives a run terminal.** `re_evaluate_after_command`
//!    (`extend_cycle` / `bump_region` / `end_region` / `resume_run`) can drive a
//!    run `Halted` while it still projects a session-holding node — a state the
//!    admission count deliberately excludes (`excludes_halted_run_with_a_running_node`),
//!    so a slot frees there too.
//!
//! The cap is set through the **stored** tier (`PUT /settings`), never
//! `PDO_SESSION_CAP`: that env var is process-global and would race every sibling
//! test. Stored beats env, so this is hermetic even under a runner that exports one.

use std::time::Duration;

use crate::common::TestDaemon;
use pdo_daemon::tmux_session_manager;

const PIPELINE_NAME: &str = "starve";

/// `start` fans out to two `doc-only` nodes that both go `Running` the instant a
/// Run is created — each consuming an admission slot — so a single Run of this
/// pipeline saturates a cap of 2. `leader` alone feeds `end`; `leaf1` is a leaf,
/// which is what lets us kill it while `leader` stays alive.
const PIPELINE_YAML: &str = r#"name: starve
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: leaf1
    name: leaf1
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: leader
    name: leader
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: leaf1, port: task }
  - source: { node: start, port: user_prompt }
    target: { node: leader, port: task }
  - source: { node: leader, port: plan }
    target: { node: end, port: result }
"#;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

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

async fn set_session_cap(daemon: &TestDaemon, cap: u32) {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "session_cap": cap }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PUT /settings session_cap={cap}");
}

async fn create_run(daemon: &TestDaemon) -> String {
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should succeed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn node_status(daemon: &TestDaemon, run_id: &str, node: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["nodes"][node]["status"].as_str().map(String::from)
}

async fn wait_for_node_status(daemon: &TestDaemon, run_id: &str, node: &str, want: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if node_status(daemon, run_id, node).await.as_deref() == Some(want) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "node {node} of run {run_id} never reached {want:?} (last = {:?})",
        node_status(daemon, run_id, node).await
    );
}

/// #509 — the boot-recovery half, the path reproduced on the issue.
///
/// Two Runs of a fan-out pipeline share a cap of 2: Run A takes both slots, Run B
/// is throttled into `waiting`. An external crash kills ONE of Run A's two node
/// sessions (`leaf1`) while `leader` stays live. Boot recovery fails the orphan —
/// freeing its slot — but Run A never stalls at the run level (its `leader` is
/// still Running), so before the fix nothing re-drove Run B and it starved for
/// ever. The fix re-drives the queue once, after the whole recovery pass.
#[tokio::test]
async fn boot_recovery_redrives_a_waiting_node_after_freeing_a_slot() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let socket = daemon.tmux_socket();
    set_session_cap(&daemon, 2).await;

    // Run A saturates the cap: leaf1 + leader both Running (2/2 slots).
    let run_a = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_a, "leaf1", "running").await;
    wait_for_node_status(&daemon, &run_a, "leader", "running").await;

    // Run B is refused admission on both entry nodes → waiting.
    let run_b = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_b, "leaf1", "waiting").await;
    wait_for_node_status(&daemon, &run_b, "leader", "waiting").await;

    // Simulate the crash: kill ONLY leaf1's session in Run A, out of band. leader
    // stays alive and attached.
    let leaf1_session = tmux_session_manager::node_session_name(&run_a, "leaf1", 1);
    tmux_session_manager::kill(&socket, &leaf1_session);
    tokio::time::sleep(Duration::from_millis(200)).await;

    daemon.run_boot_recovery_tick().await;

    // Run A: the orphan is Interrupted (ADR-0049); the sibling and the Run itself
    // stay live — so the run-level stall reconciliation never fires for Run A.
    assert_eq!(
        node_status(&daemon, &run_a, "leaf1").await.as_deref(),
        Some("interrupted"),
        "the orphaned node must be Interrupted at boot (ADR-0049)"
    );
    assert_eq!(
        node_status(&daemon, &run_a, "leader").await.as_deref(),
        Some("running"),
        "the live sibling must stay Running (it suppresses run-level reconciliation)"
    );

    // The freed slot must be redistributed to Run B. With cap 2 and one live
    // session left (leader A), exactly one of Run B's two queued nodes wins the
    // slot; the other stays waiting. Before the fix, BOTH stayed waiting for ever.
    wait_for_a_redriven_node(&daemon, &run_b).await;

    tmux_session_manager::kill(
        &socket,
        &tmux_session_manager::node_session_name(&run_a, "leader", 1),
    );
    for node in ["leaf1", "leader"] {
        tmux_session_manager::kill(
            &socket,
            &tmux_session_manager::node_session_name(&run_b, node, 1),
        );
    }
}

async fn wait_for_a_redriven_node(daemon: &TestDaemon, run_b: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let leaf1 = node_status(daemon, run_b, "leaf1").await;
        let leader = node_status(daemon, run_b, "leader").await;
        if leaf1.as_deref() == Some("running") || leader.as_deref() == Some("running") {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "the freed slot was never redistributed to a waiting node \
                 (leaf1={leaf1:?}, leader={leader:?}) — #509 starvation"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
