//! Layer 4 (ADR-0080) — the Run routes no integration test called before the
//! refactor net (#914): the reapable listing, the costs of the listed Runs and
//! the progress the list carries, the Pipeline Manager on demand
//! (start, stop, attach), the release of an interactive node's completion, the
//! Run's refs and its drift from the source branch, and the terminal pop-out of
//! a session.
//!
//! The two `…/attach` routes open an OS terminal. `PDO_TERMINAL` points them at a
//! recording stub under the test's tempdir, so no window ever opens.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::flow;
use crate::common::{lock_terminal, TestDaemon};

const SOLO: &str = "solo";
const GATED: &str = "gated";

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[(SOLO, false), (GATED, true)])
}

async fn post(daemon: &TestDaemon, path: &str) -> (u16, serde_json::Value) {
    flow::post_json(daemon, path, serde_json::json!({})).await
}

// --- GET /runs/reapable -----------------------------------------------------

#[tokio::test]
async fn reapable_lists_terminal_runs_whose_worktree_is_still_on_disk() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let done = flow::completed_solo_run(&daemon, SOLO).await;
    let live = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &live, "worker", "running").await;

    let list = daemon.get_json("/runs/reapable").await;
    let entries = list.as_array().unwrap();
    assert_eq!(entries.len(), 1, "only the terminal run: {list}");
    let entry = &entries[0];
    assert_eq!(entry["run_id"], done.as_str());
    assert_eq!(entry["pipeline_name"], SOLO);
    assert_eq!(entry["status"], "completed");
    assert_eq!(entry["worktree_present"], true);
    assert_eq!(entry["effective_repo"], daemon.target_repo().as_str());
    assert!(entry["age_secs"].as_i64().unwrap() >= 0, "{entry}");
    assert!(
        entry["approx_disk_bytes"].is_null(),
        "size is opt-in: {entry}"
    );

    let sized = daemon.get_json("/runs/reapable?size=true").await;
    assert!(
        sized[0]["approx_disk_bytes"].as_u64().unwrap() > 0,
        "{sized}"
    );

    // Once the worktree is gone there is nothing left to reap.
    std::fs::remove_dir_all(flow::worktree(&daemon, &done)).unwrap();
    std::fs::remove_dir_all(
        daemon
            .repo_root()
            .join(".pdo/runs")
            .join(&done)
            .join("nodes"),
    )
    .ok();
    let list = daemon.get_json("/runs/reapable").await;
    assert_eq!(list, serde_json::json!([]));
}

// --- GET /runs: the row's progress and duration; GET /runs/costs ------------

#[tokio::test]
async fn the_list_carries_each_run_s_working_nodes_and_its_end() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let done = flow::completed_solo_run(&daemon, SOLO).await;
    let live = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &live, "worker", "running").await;

    let list = daemon.get_json("/runs").await;
    let entry = |id: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|r| r["run_id"] == id)
            .cloned()
            .unwrap()
    };
    let finished = entry(&done);
    assert!(finished["completed_at"].is_string(), "{finished}");
    assert_eq!(finished["nodes"][0]["id"], "worker", "{finished}");
    assert_eq!(finished["nodes"][0]["status"], "completed", "{finished}");
    let running = entry(&live);
    assert!(running.get("completed_at").is_none(), "{running}");
    assert_eq!(running["nodes"][0]["status"], "running", "{running}");
}

#[tokio::test]
async fn costs_answers_for_the_runs_asked_that_exist_in_the_order_asked() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let done = flow::completed_solo_run(&daemon, SOLO).await;

    let costs = daemon
        .get_json(&format!("/runs/costs?ids=no-such-run,{done},{done}"))
        .await;
    let runs = costs["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "{costs}");
    assert_eq!(runs[0]["run_id"], done.as_str());

    let none = daemon.get_json("/runs/costs").await;
    assert_eq!(none, serde_json::json!({ "runs": [] }));

    let many = (0..101)
        .map(|i| format!("r{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let resp = reqwest::get(format!("{}/runs/costs?ids={many}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

// --- POST /runs/{id}/manager/start|stop --------------------------------------

#[tokio::test]
async fn manager_start_is_idempotent_and_stop_kills_the_session() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    let session = format!("pdo-mgr-{run_id}");
    assert!(
        !flow::tmux_session_exists(&daemon, &session),
        "managerless by default"
    );

    let (status, body) = post(&daemon, &format!("/runs/{run_id}/manager/start")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({ "ok": true, "session": session, "created": true })
    );
    assert!(flow::tmux_session_exists(&daemon, &session));
    flow::wait_run(&daemon, &run_id, "has_manager", |run| {
        run["has_manager"] == true
    })
    .await;

    let (status, body) = post(&daemon, &format!("/runs/{run_id}/manager/start")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["created"], false, "a double-click re-answers: {body}");

    let (status, body) = post(&daemon, &format!("/runs/{run_id}/manager/stop")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, serde_json::json!({ "ok": true, "stopped": true }));
    assert!(!flow::tmux_session_exists(&daemon, &session));
    flow::wait_run(&daemon, &run_id, "no manager", |run| {
        run["has_manager"] == false
    })
    .await;

    let (status, body) = post(&daemon, &format!("/runs/{run_id}/manager/stop")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({ "ok": true, "stopped": false }),
        "nothing to stop is a calm no-op"
    );

    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    let kinds: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"manager_started"), "{kinds:?}");
    assert_eq!(
        kinds.iter().filter(|k| **k == "manager_stopped").count(),
        2,
        "both stops are recorded: {kinds:?}"
    );
}

#[tokio::test]
async fn manager_start_refuses_an_unknown_run_and_a_run_without_worktree() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, _) = post(&daemon, "/runs/no-such-run/manager/start").await;
    assert_eq!(status, 404);

    let run_id = flow::completed_solo_run(&daemon, SOLO).await;
    std::fs::remove_dir_all(flow::worktree(&daemon, &run_id)).unwrap();
    let (status, body) = post(&daemon, &format!("/runs/{run_id}/manager/start")).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("worktree no longer exists"),
        "{body}"
    );
}

// --- POST /runs/{id}/nodes/{nid}/release -------------------------------------

#[tokio::test]
async fn release_lets_an_interactive_node_complete() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, GATED).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;

    let (status, body) = flow::complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 409, "held until released: {body}");
    assert_eq!(body["error"], "completion_not_released", "{body}");

    let release = format!("/runs/{run_id}/nodes/worker/release");
    let (status, body) = flow::post_json(&daemon, &release, serde_json::json!({ "iter": 1 })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, serde_json::json!({ "ok": true, "released": true }));

    let (status, body) = flow::post_json(&daemon, &release, serde_json::json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({ "ok": true, "released": true, "noop": true })
    );

    let (status, body) = flow::complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 200, "released, the completion goes through: {body}");
    flow::wait_node_status(&daemon, &run_id, "worker", "completed").await;
    flow::wait_run_status(&daemon, &run_id, "completed").await;

    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "node_completion_released" && e["node_id"] == "worker"));
}

#[tokio::test]
async fn release_refuses_what_it_cannot_release() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = post(&daemon, "/runs/no-such-run/nodes/worker/release").await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "run_not_found");

    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    let (status, body) = post(&daemon, &format!("/runs/{run_id}/nodes/ghost/release")).await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "node_not_found");

    let (status, body) = post(&daemon, &format!("/runs/{run_id}/nodes/worker/release")).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_not_interactive");

    let (status, body) = flow::post_json(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/release"),
        serde_json::json!({ "iter": 7 }),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");

    let (status, _) = flow::complete_node(&daemon, &run_id, "worker").await;
    assert_eq!(status, 200);
    flow::wait_run_status(&daemon, &run_id, "completed").await;
    let (status, body) = post(&daemon, &format!("/runs/{run_id}/nodes/worker/release")).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "run_not_live");
}

// --- GET /runs/{id}/refs, GET /runs/{id}/source-drift -------------------------

#[tokio::test]
async fn refs_name_the_fork_the_tip_and_the_worktree_with_resolved_shas() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;

    let refs = daemon.get_json(&format!("/runs/{run_id}/refs")).await;
    let ids: Vec<&str> = refs["refs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(ids.first(), Some(&"fork"), "{refs}");
    assert!(ids.contains(&"tip"), "{refs}");
    assert!(ids.contains(&"worktree"), "{refs}");
    assert_eq!(refs["default_from"], "fork");

    let tip = refs["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "tip")
        .unwrap();
    let head = flow::git(
        daemon.repo_root(),
        &["rev-parse", &format!("pdo/run-{run_id}")],
    );
    assert_eq!(tip["sha"], head.as_str(), "{tip}");
    assert!(refs["deliveries"].as_array().unwrap().is_empty(), "{refs}");

    let resp = reqwest::get(format!("{}/runs/no-such-run/refs", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn source_drift_counts_what_the_run_made_and_what_arrived_on_its_source() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": SOLO,
            "input": "go",
            "target_repo": daemon.target_repo(),
            "source_branch": "main",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;

    let drift = daemon
        .get_json(&format!("/runs/{run_id}/source-drift"))
        .await;
    assert_eq!(drift["state"], "available", "{drift}");
    assert_eq!(drift["source_branch"], "main");
    assert_eq!(drift["ahead"], 0);
    assert_eq!(drift["behind"], 0);

    // One commit on the Run's branch, two on the source.
    let wt = flow::worktree(&daemon, &run_id);
    flow::git(&wt, &["commit", "-q", "--allow-empty", "-m", "run work"]);
    let repo = daemon.repo_root();
    flow::git(repo, &["commit", "-q", "--allow-empty", "-m", "source 1"]);
    flow::git(repo, &["commit", "-q", "--allow-empty", "-m", "source 2"]);

    let drift = daemon
        .get_json(&format!("/runs/{run_id}/source-drift"))
        .await;
    assert_eq!(drift["state"], "available", "{drift}");
    assert_eq!(drift["ahead"], 1, "{drift}");
    assert_eq!(drift["behind"], 2, "{drift}");
    assert_eq!(drift["local_behind"], 2, "{drift}");
    assert!(drift["upstream"].is_null(), "no tracking branch: {drift}");

    // The source branch disappears: unavailable, still 200.
    flow::git(repo, &["checkout", "-q", "-b", "elsewhere"]);
    flow::git(repo, &["branch", "-q", "-D", "main"]);
    let drift = daemon
        .get_json(&format!("/runs/{run_id}/source-drift"))
        .await;
    assert_eq!(
        drift,
        serde_json::json!({
            "state": "unavailable",
            "reason": "the source branch no longer exists",
        })
    );

    let resp = reqwest::get(format!("{}/runs/no-such-run/source-drift", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// --- POST /sessions/{sid}/attach, POST /sessions/{run_id}/manager/attach ------

/// A terminal stand-in that writes the argv it was given to `<dir>/argv`.
fn recording_terminal(dir: &Path) -> (PathBuf, PathBuf) {
    let script = dir.join("fake-terminal.sh");
    let argv = dir.join("argv");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", argv.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (script, argv)
}

async fn read_argv(argv: &Path) -> String {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(argv) {
            if !text.is_empty() {
                return text;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the terminal stub was never launched ({})", argv.display());
}

#[tokio::test]
async fn session_attach_pops_the_configured_terminal_on_the_daemon_socket() {
    let stubs = tempfile::tempdir().unwrap();
    let (script, argv) = recording_terminal(stubs.path());
    let _terminal = lock_terminal(script.to_string_lossy());
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = post(&daemon, "/sessions/pdo-some-session/attach").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({
            "ok": true,
            "session": "pdo-some-session",
            "terminal": script.to_string_lossy(),
        })
    );
    let args = read_argv(&argv).await;
    assert!(
        args.contains(&format!(
            "tmux -L '{}' attach -t 'pdo-some-session'",
            daemon.tmux_socket()
        )),
        "{args}"
    );
}

#[tokio::test]
async fn session_attach_reports_a_terminal_that_cannot_start() {
    let _terminal = lock_terminal("/nonexistent/pdo-terminal");
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = post(&daemon, "/sessions/pdo-some-session/attach").await;
    assert_eq!(status, 500, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("failed to attach:"),
        "{body}"
    );
}

#[tokio::test]
async fn manager_attach_needs_a_live_manager_then_pops_the_terminal_on_it() {
    let stubs = tempfile::tempdir().unwrap();
    let (script, argv) = recording_terminal(stubs.path());
    let _terminal = lock_terminal(script.to_string_lossy());
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = flow::create_run(&daemon, SOLO).await;
    flow::wait_node_status(&daemon, &run_id, "worker", "running").await;
    let session = format!("pdo-mgr-{run_id}");

    let (status, body) = post(&daemon, &format!("/sessions/{run_id}/manager/attach")).await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(
        body["error"],
        format!("manager session {session} not found").as_str()
    );

    let (status, _) = post(&daemon, &format!("/runs/{run_id}/manager/start")).await;
    assert_eq!(status, 200);
    let (status, body) = post(&daemon, &format!("/sessions/{run_id}/manager/attach")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["session"], session.as_str());
    let args = read_argv(&argv).await;
    assert!(
        args.contains(&format!(
            "tmux -L '{}' attach -t '{session}'",
            daemon.tmux_socket()
        )),
        "{args}"
    );
}
