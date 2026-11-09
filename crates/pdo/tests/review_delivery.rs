//! Layer 3 — CORE-239: a batch of review comments on its way to the Pipeline
//! Manager, over a real daemon and a real tmux.
//!
//! - The manager a send starts reads the batch in its launch prompt: nothing
//!   is typed into a harness that is still starting.
//! - Its session ends before answering (here: killed, as a harness that
//!   exits at once): the comment says so (`delivery.state == "exited"`), and
//!   « Send again » starts a new manager with the comment as written.
//! - A manager whose screen shows its agent's folder-trust prompt holds the
//!   batch (`blocked`, with the way out), and one that exits on it says so.

use std::time::Duration;

use serde_json::{json, Value};

use crate::common::TestDaemon;
use crate::review_comments::{create_run, git, seed};

/// A Run with one change on its branch, its worktree ready.
async fn run_with_a_change(daemon: &TestDaemon) -> (String, std::path::PathBuf) {
    let run_id = create_run(daemon.url(), daemon.target_repo())
        .await
        .expect("run created");
    let wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree");
    for _ in 0..100 {
        if wt_dir.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    std::fs::write(
        wt_dir.join("lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    git(&wt_dir, &["add", "lib.rs"]);
    git(&wt_dir, &["commit", "-q", "-m", "run work"]);
    (run_id, wt_dir)
}

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// The first comment's `delivery`, once `state` (or nothing, for `None`)
/// shows within a few seconds — `Value::Null` for none.
async fn delivery_becomes(daemon: &TestDaemon, run_id: &str, state: Option<&str>) -> Value {
    let mut last = Value::Null;
    for _ in 0..80 {
        let run = daemon.get_json(&format!("/runs/{run_id}")).await;
        last = run["review_comments"][0]["delivery"].clone();
        if last["state"].as_str() == state {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("delivery never became {state:?}: last {last}");
}

fn session_up(daemon: &TestDaemon, session: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", &daemon.tmux_socket(), "has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn kill_session(daemon: &TestDaemon, session: &str) {
    std::process::Command::new("tmux")
        .args(["-L", &daemon.tmux_socket(), "kill-session", "-t", session])
        .output()
        .unwrap();
}

fn comment() -> Value {
    json!({ "comments": [{ "path": "lib.rs", "side": "new", "line": 4, "text": "Name this `fn delta`." }] })
}

#[tokio::test]
async fn a_manager_that_exits_before_answering_is_said_and_send_again_restarts_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (run_id, wt_dir) = run_with_a_change(&daemon).await;
    let session = format!("pdo-mgr-{run_id}");

    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/review/comments/send"),
        comment(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["manager_started"], true);
    // The batch is in the launch prompt of the manager the send started.
    let prompt =
        std::fs::read_to_string(wt_dir.join(".pdo/prompts/__manager__-iter-0.md")).unwrap();
    assert!(prompt.contains("[pdo-runtime] Review comments"), "{prompt}");
    assert!(prompt.contains("[rc-001] lib.rs:R4"), "{prompt}");
    assert!(session_up(&daemon, &session));
    delivery_becomes(&daemon, &run_id, None).await;

    // The session ends before the manager answers.
    kill_session(&daemon, &session);
    let held = delivery_becomes(&daemon, &run_id, Some("exited")).await;
    assert!(held.get("reason").is_none(), "{held}");

    // « Send again »: a new manager, the comment as written in its prompt.
    let _ = std::fs::remove_file(wt_dir.join(".pdo/prompts/__manager__-iter-0.md"));
    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/review/comments/resend"),
        json!({ "ids": ["rc-001"] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["manager_started"], true);
    assert!(session_up(&daemon, &session));
    let prompt =
        std::fs::read_to_string(wt_dir.join(".pdo/prompts/__manager__-iter-0.md")).unwrap();
    assert!(prompt.contains("Name this `fn delta`."), "{prompt}");
    delivery_becomes(&daemon, &run_id, None).await;

    let events: Vec<Value> = reqwest::Client::new()
        .get(format!("{}/runs/{run_id}/events", daemon.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let states: Vec<&str> = events
        .iter()
        .filter(|e| e["kind"] == "review_comment_delivery")
        .map(|e| e["payload"]["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, vec!["exited", "awaiting"]);
}

#[tokio::test]
async fn a_manager_on_its_trust_prompt_holds_the_batch_and_says_how_to_unblock() {
    // Every session shows the agent's folder-trust prompt, then waits.
    let daemon = TestDaemon::spawn_with_override(
        seed,
        Some(
            "printf 'Quick safety check\\nDo you trust the files in this folder?\\n 1. No, exit\\n 2. Yes, I trust this folder\\n'; exec sleep 600"
                .to_string(),
        ),
    )
    .await
    .unwrap();
    let (run_id, _) = run_with_a_change(&daemon).await;
    let (status, body) = post(
        &daemon,
        &format!("/runs/{run_id}/review/comments/send"),
        comment(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let held = delivery_becomes(&daemon, &run_id, Some("blocked")).await;
    let reason = held["reason"].as_str().unwrap();
    assert!(reason.contains("Manager tab"), "{reason}");
    assert!(reason.contains("Yes, I trust this folder"), "{reason}");

    // « No, exit »: the session ends on the prompt.
    kill_session(&daemon, &format!("pdo-mgr-{run_id}"));
    let held = delivery_becomes(&daemon, &run_id, Some("exited")).await;
    assert!(
        held["reason"]
            .as_str()
            .unwrap()
            .contains("folder-trust prompt"),
        "{held}"
    );
}
