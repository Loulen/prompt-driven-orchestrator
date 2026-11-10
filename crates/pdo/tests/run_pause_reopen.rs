//! Layer 3a — what a pause and a reopen leave of the work in flight, against a
//! real daemon:
//!
//! - a node that finishes while its Run is paused completes (a `script` calls
//!   `pdo complete` once, as its bash exits: a refusal lost its work), and what
//!   it would launch waits for the resume (the Run ends then if it was the
//!   last);
//! - reopening a cancelled Run runs again the nodes the cancel stopped, instead
//!   of parking it « run_stalled » behind them.

use crate::common::flow::{self, wait_node_status, wait_run_status};
use crate::common::TestDaemon;

/// `start → worker → second → end`, two non-isolated agents.
fn duo_yaml() -> String {
    r#"name: duo
version: "1.0"
nodes:
  - { id: start, name: Start, type: start, outputs: [ { name: user_prompt } ] }
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs: [ { name: task } ]
    outputs: [ { name: out } ]
  - id: second
    name: Second
    type: agent
    isolated_worktree: false
    inputs: [ { name: task } ]
    outputs: [ { name: out } ]
  - { id: end, name: End, type: end, inputs: [ { name: result } ] }
edges:
  - { source: { node: start, port: user_prompt }, target: { node: worker, port: task } }
  - { source: { node: worker, port: out }, target: { node: second, port: task } }
  - { source: { node: second, port: out }, target: { node: end, port: result } }
"#
    .to_string()
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::write_pipeline(repo, "duo", &duo_yaml())?;
    std::fs::write(
        repo.join(".pdo/pipelines/duo.prompts/second.md"),
        "Do the rest.\n",
    )?;
    flow::seed_solo(repo, &[("solo", false)])
}

async fn command(daemon: &TestDaemon, run_id: &str, kind: &str) -> (u16, serde_json::Value) {
    flow::post_json(
        daemon,
        &format!("/runs/{run_id}/commands"),
        serde_json::json!({ "kind": kind }),
    )
    .await
}

#[tokio::test]
async fn a_node_finishing_during_a_pause_completes_and_its_successor_waits_for_the_resume() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "duo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let (status, body) = command(&daemon, &run_id, "pause_run").await;
    assert_eq!(status, 200, "pause_run: {body}");
    wait_run_status(&daemon, &run_id, "paused").await;

    let (status, body) = flow::complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 200, "a completion lands on a paused Run: {body}");
    let run = wait_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["status"], "paused", "the Run stays paused: {run}");

    // Nothing new is launched while the Run is paused.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert!(
        run["nodes"].get("second").is_none_or(|n| n.is_null()),
        "second waits for the resume: {run}"
    );

    let (status, body) = command(&daemon, &run_id, "resume_run").await;
    assert_eq!(status, 200, "resume_run: {body}");
    wait_node_status(&daemon, &run_id, "second", "running").await;
}

#[tokio::test]
async fn a_run_whose_last_node_finishes_during_a_pause_completes_on_resume() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let (status, body) = command(&daemon, &run_id, "pause_run").await;
    assert_eq!(status, 200, "pause_run: {body}");
    wait_run_status(&daemon, &run_id, "paused").await;
    let (status, body) = flow::complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 200, "a completion lands on a paused Run: {body}");
    let run = wait_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["status"], "paused", "a paused Run does not end: {run}");

    let (status, body) = command(&daemon, &run_id, "resume_run").await;
    assert_eq!(status, 200, "resume_run: {body}");
    wait_run_status(&daemon, &run_id, "completed").await;
}

#[tokio::test]
async fn reopening_a_cancelled_run_runs_the_stopped_node_again() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, "solo").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let (status, body) = command(&daemon, &run_id, "cancel_run").await;
    assert_eq!(status, 200, "cancel_run: {body}");
    let run = wait_run_status(&daemon, &run_id, "halted").await;
    assert_eq!(run["nodes"]["worker"]["status"], "stopped");

    let (status, body) = command(&daemon, &run_id, "reopen_run").await;
    assert_eq!(status, 200, "reopen_run: {body}");
    let run = wait_node_status(&daemon, &run_id, "worker", "running").await;
    assert_eq!(run["status"], "running", "{run}");
    assert!(
        flow::node_is_live(&daemon, &run_id, "worker").await,
        "the stopped worker has a session again"
    );

    // And the Run goes to its end from there.
    let iter = run["nodes"]["worker"]["iter"].as_i64().unwrap();
    flow::write_out_artifact_at(&daemon, &run_id, "worker", iter);
    let (status, body) = flow::post_json(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/done"),
        serde_json::json!({ "iter": iter }),
    )
    .await;
    assert_eq!(status, 200, "worker completes: {body}");
    wait_run_status(&daemon, &run_id, "completed").await;
}
