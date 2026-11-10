//! Layer 3 — a node's **Transcript** through a real daemon:
//! `GET /runs/{run_id}/nodes/{node_id}/transcript` and its tool detail
//! `GET /runs/{run_id}/nodes/{node_id}/transcript/tools/{index}`.
//!
//! Proven here rather than in the harness context's unit tests: the route finds
//! the file the iteration's **frozen launch** names (harness, pinned session,
//! working directory, store root), pages it, picks up what a live session
//! appends, reads a tool call on demand, and answers « unavailable » — not an
//! error — for a harness PDO reads no transcript of, and for a script node.

use crate::common::TestDaemon;
use std::process::Command;

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn pipeline(name: &str, harness: &str) -> String {
    format!(
        r#"name: {name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    pin_harness: {harness}
    inputs:
      - {{ name: task, side: top }}
    outputs:
      - {{ name: out, side: bottom }}
  - id: end
    name: End
    type: end
    inputs:
      - {{ name: result, side: top }}
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: worker, port: task }}
  - source: {{ node: worker, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines)?;
    for (name, harness) in [
        ("transcript-claude", "claude"),
        ("transcript-opencode", "opencode"),
    ] {
        std::fs::write(
            pipelines.join(format!("{name}.yaml")),
            pipeline(name, harness),
        )?;
        let prompts = pipelines.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts)?;
        std::fs::write(prompts.join("worker.md"), "Work.\n")?;
    }
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "t@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": pipeline,
            "input": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn wait_for_started(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    for _ in 0..100 {
        let evs: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if evs
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{node_id} should have started within the timeout");
}

fn transcript_path(daemon: &TestDaemon, run_id: &str, session_id: &str) -> std::path::PathBuf {
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_harness::claude_transcript::encode_working_dir(&worktree);
    let dir = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{session_id}.jsonl"))
}

const SESSION: &str = r#"{"type":"user","uuid":"u1","timestamp":"2026-10-19T10:42:03Z","message":{"role":"user","content":"Which file holds the spec?"}}
{"type":"assistant","uuid":"a1","timestamp":"2026-10-19T10:42:05Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"HIDDEN-THOUGHT"}]}}
{"type":"assistant","uuid":"a2","timestamp":"2026-10-19T10:42:06Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"spec.md"}}]}}
{"type":"user","uuid":"u2","timestamp":"2026-10-19T10:42:07Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Add a debounce."}]}}
{"type":"assistant","uuid":"a3","timestamp":"2026-10-19T10:42:09Z","message":{"role":"assistant","content":[{"type":"text","text":"spec.md asks for a debounce."}]}}
"#;

async fn get(daemon: &TestDaemon, path: String) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .get(format!("{}{path}", daemon.url()))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn a_claude_node_pages_its_transcript_and_opens_a_tool_call() {
    if !tmux_available() {
        return; // the node must spawn to pin its session
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let run_id = create_run(&daemon, "transcript-claude").await;
    wait_for_started(&daemon, &run_id, "worker").await;

    // Started, nothing written yet: readable, empty.
    let (status, page) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=1"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(page["available"], true);
    assert_eq!(page["total"], 0);

    let sid = daemon.pinned_session_id(&run_id, "worker").await;
    let path = transcript_path(&daemon, &run_id, &sid);
    std::fs::write(&path, SESSION).unwrap();

    let (status, page) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=1&after=0&limit=2"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(page["harness"], "claude");
    assert_eq!(
        (
            page["total"].as_u64(),
            page["messages"].as_u64(),
            page["tool_calls"].as_u64()
        ),
        (Some(3), Some(2), Some(1))
    );
    assert_eq!(page["next"], 2);
    let entries = page["entries"].as_array().unwrap();
    assert_eq!(entries[0]["kind"], "message");
    assert_eq!(entries[0]["speaker"], "prompt");
    assert_eq!(entries[0]["text"], "Which file holds the spec?");
    assert_eq!(entries[1]["kind"], "tool");
    assert_eq!(entries[1]["name"], "Read");
    assert_eq!(entries[1]["summary"], "spec.md");
    assert_eq!(entries[1]["status"], "ok");
    assert!(
        entries[1].get("input").is_none(),
        "a folded call carries no input"
    );
    assert!(!page.to_string().contains("HIDDEN-THOUGHT"));

    let (_, rest) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=1&after=2"),
    )
    .await;
    assert_eq!(rest["entries"][0]["speaker"], "agent");
    assert!(rest.get("next").is_none());

    // The tool call, on demand.
    let (status, detail) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript/tools/1?iter=1"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(detail["name"], "Read");
    assert!(detail["input"].as_str().unwrap().contains("spec.md"));
    assert_eq!(detail["output"], "Add a debounce.");
    let (status, _) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript/tools/0?iter=1"),
    )
    .await;
    assert_eq!(status, 404, "a message is not a tool call");

    // Live: what the session appends arrives from where the reader stopped.
    let mut grown = SESSION.to_string();
    grown.push_str(r#"{"type":"user","uuid":"u3","timestamp":"2026-10-19T10:43:00Z","message":{"role":"user","content":"Thanks."}}"#);
    grown.push('\n');
    std::fs::write(&path, grown).unwrap();
    let (_, live) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=1&after=3"),
    )
    .await;
    assert_eq!(live["total"], 4);
    assert_eq!(live["entries"].as_array().unwrap().len(), 1);
    assert_eq!(live["entries"][0]["text"], "Thanks.");

    // Another iteration than the one started: nothing written.
    let (_, other) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=2"),
    )
    .await;
    assert_eq!(other["available"], true);
    assert_eq!(other["total"], 0);

    // A structural node runs no agent; an unknown node is not found.
    let (_, start) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/start/transcript?iter=1"),
    )
    .await;
    assert_eq!(start["available"], false);
    let (status, _) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/nope/transcript?iter=1"),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_harness_without_a_reader_is_unavailable_not_an_error() {
    if !tmux_available() {
        return;
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let run_id = create_run(&daemon, "transcript-opencode").await;
    wait_for_started(&daemon, &run_id, "worker").await;
    let (status, page) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript?iter=1"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(page["available"], false);
    assert_eq!(page["harness"], "opencode");
    assert_eq!(page["entries"].as_array().unwrap().len(), 0);
    let (status, _) = get(
        &daemon,
        format!("/runs/{run_id}/nodes/worker/transcript/tools/0?iter=1"),
    )
    .await;
    assert_eq!(status, 404);
}
