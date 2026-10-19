//! Layer 3 — the search over a Run's **Transcripts** through a real daemon:
//! `GET /runs/{run_id}/transcripts/search?q=&client=`.
//!
//! Two agent nodes run side by side; each writes a transcript where its
//! frozen launch says. The search finds the word in both, by node then
//! chronology, with an excerpt, counts the matches per node, says so when
//! nothing matches, and never reads a thought of the model.

use crate::common::TestDaemon;
use std::process::Command;

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

const PIPELINE: &str = r#"name: search-two
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: builder
    name: builder
    type: agent
    isolated_worktree: false
    pin_harness: claude
    inputs:
      - name: task
    outputs:
      - name: out
  - id: reviewer
    name: reviewer
    type: agent
    isolated_worktree: false
    pin_harness: claude
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: builder, port: task }
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(pipelines.join("search-two.prompts"))?;
    std::fs::write(pipelines.join("search-two.yaml"), PIPELINE)?;
    for node in ["builder", "reviewer"] {
        std::fs::write(
            pipelines
                .join("search-two.prompts")
                .join(format!("{node}.md")),
            "Work.\n",
        )?;
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

async fn started(daemon: &TestDaemon, run_id: &str, node_id: &str) -> bool {
    let evs: serde_json::Value = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    evs.as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
}

fn plant(daemon: &TestDaemon, run_id: &str, session_id: &str, lines: &str) {
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_harness::claude_transcript::encode_working_dir(&worktree);
    let dir = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{session_id}.jsonl")), lines).unwrap();
}

const BUILDER: &str = r#"{"type":"user","uuid":"u1","timestamp":"2026-10-19T10:42:03Z","message":{"role":"user","content":"Implement the search filter."}}
{"type":"assistant","uuid":"a1","timestamp":"2026-10-19T10:42:05Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"a secret debounce plan"}]}}
{"type":"assistant","uuid":"a2","timestamp":"2026-10-19T10:43:40Z","message":{"role":"assistant","content":[{"type":"text","text":"The spec asks for a debounce on the input."}]}}
{"type":"assistant","uuid":"a3","timestamp":"2026-10-19T10:44:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"src/useDebounce.ts","old_string":"a","new_string":"b"}}]}}
"#;

const REVIEWER: &str = r#"{"type":"assistant","uuid":"r1","timestamp":"2026-10-19T11:02:00Z","message":{"role":"assistant","content":[{"type":"text","text":"The debounce is not covered by a test."}]}}
"#;

async fn search(daemon: &TestDaemon, run_id: &str, q: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/transcripts/search", daemon.url()))
        .query(&[("q", q), ("client", "box-1")])
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn a_run_search_finds_a_word_in_two_nodes() {
    if !tmux_available() {
        return; // the nodes must spawn to pin their sessions
    }
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": "search-two",
            "input": "go",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..100 {
        if started(&daemon, &run_id, "builder").await && started(&daemon, &run_id, "reviewer").await
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Nothing written yet: no match, not an error.
    let (status, empty) = search(&daemon, &run_id, "debounce").await;
    assert_eq!(status, 200);
    assert_eq!(empty["matched"], 0);
    assert_eq!(empty["hits"].as_array().unwrap().len(), 0);

    let builder = daemon.pinned_session_id(&run_id, "builder").await;
    let reviewer = daemon.pinned_session_id(&run_id, "reviewer").await;
    plant(&daemon, &run_id, &builder, BUILDER);
    plant(&daemon, &run_id, &reviewer, REVIEWER);

    let (status, found) = search(&daemon, &run_id, "  DEBOUNCE ").await;
    assert_eq!(status, 200);
    assert_eq!(found["query"], "DEBOUNCE");
    assert_eq!(
        found["matched"], 3,
        "two builder entries, one reviewer entry"
    );
    let hits = found["hits"].as_array().unwrap();
    let shape: Vec<String> = hits
        .iter()
        .map(|h| {
            format!(
                "{}#{}:{}:{}",
                h["node_id"].as_str().unwrap(),
                h["iter"],
                h["hit"]["kind"].as_str().unwrap(),
                h["hit"]["index"]
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            "builder#1:message:1",
            "builder#1:tool:2",
            "reviewer#1:message:0"
        ]
    );
    assert_eq!(hits[0]["hit"]["speaker"], "agent");
    assert_eq!(hits[0]["hit"]["snippet"]["before"], "The spec asks for a ");
    assert_eq!(hits[0]["hit"]["snippet"]["matched"], "debounce");
    assert_eq!(hits[0]["hit"]["snippet"]["after"], " on the input.");
    assert_eq!(hits[0]["hit"]["at"], "2026-10-19T10:43:40Z");
    assert_eq!(hits[1]["hit"]["name"], "Edit");
    assert_eq!(
        found["nodes"],
        serde_json::json!([
            { "node_id": "builder", "matched": 2 },
            { "node_id": "reviewer", "matched": 1 },
        ])
    );
    assert_eq!(found["truncated"], false);
    assert!(
        !found.to_string().contains("secret"),
        "a thought is never searched"
    );

    let (_, none) = search(&daemon, &run_id, "zzz-nowhere").await;
    assert_eq!(none["matched"], 0);
    assert_eq!(none["nodes"], serde_json::json!([]));

    let (status, _) = search(&daemon, &run_id, &"x".repeat(201)).await;
    assert_eq!(status, 400, "a query is bounded");
    let (status, _) = search(&daemon, "no-such-run", "debounce").await;
    assert_eq!(status, 404);
}
