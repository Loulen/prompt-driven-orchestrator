//! #971 — « Fichier importé en cours de Run », over a real daemon:
//!
//! - files imported into a node with a live session land in the Run's
//!   Blackboard under `.pdo/artifacts/_attachments/<node-id>/`, and the answer
//!   names each final path relative to the worktree root;
//! - a name already there is suffixed (`contrat-1.pdf`), never overwritten;
//! - over `max_attachments_mb` (per import): 413 naming the setting, nothing
//!   written;
//! - a node without a live session: named 409, nothing written;
//! - « Copy and send to terminal » writes the text into the session's input
//!   **without** pressing Enter, and refuses without a live session.

use std::time::Duration;

use crate::common::TestDaemon;
use crate::declared_wait::{create_run, seed, wait_node_status, PARENT};

const NODE: &str = "worker";

fn worktree(daemon: &TestDaemon, run_id: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
}

fn attachments_dir(daemon: &TestDaemon, run_id: &str, node: &str) -> std::path::PathBuf {
    worktree(daemon, run_id)
        .join(".pdo/artifacts/_attachments")
        .join(node)
}

async fn import(
    daemon: &TestDaemon,
    run_id: &str,
    node: &str,
    files: &[(&str, Vec<u8>)],
) -> (u16, serde_json::Value) {
    let mut form = reqwest::multipart::Form::new();
    for (name, data) in files {
        form = form.part(
            "files",
            reqwest::multipart::Part::bytes(data.clone()).file_name(name.to_string()),
        );
    }
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node}/attachments",
            daemon.url()
        ))
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

async fn send_text(
    daemon: &TestDaemon,
    run_id: &str,
    node: &str,
    text: &str,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node}/terminal-text",
            daemon.url()
        ))
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

fn tmux(daemon: &TestDaemon, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .arg("-L")
        .arg(daemon.tmux_socket())
        .args(args)
        .output()
        .expect("tmux runs");
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[tokio::test]
async fn imported_files_land_in_the_blackboard_and_a_taken_name_is_suffixed() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, NODE, "running").await;

    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[
            ("contrat.pdf", b"%PDF-1 first".to_vec()),
            ("annexe.csv", b"a,b\n".to_vec()),
        ],
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        body["files"][0]["path"],
        format!(".pdo/artifacts/_attachments/{NODE}/contrat.pdf")
    );
    assert_eq!(
        body["files"][1]["path"],
        format!(".pdo/artifacts/_attachments/{NODE}/annexe.csv")
    );
    // The returned path resolves from the worktree root.
    let wt = worktree(&daemon, &run_id);
    let path = body["files"][0]["path"].as_str().unwrap();
    assert_eq!(std::fs::read(wt.join(path)).unwrap(), b"%PDF-1 first");

    // Same name again: suffixed, the first one untouched.
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[("contrat.pdf", b"%PDF-1 second".to_vec())],
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["files"][0]["name"], "contrat-1.pdf");
    assert_eq!(
        body["files"][0]["path"],
        format!(".pdo/artifacts/_attachments/{NODE}/contrat-1.pdf")
    );
    let dir = attachments_dir(&daemon, &run_id, NODE);
    assert_eq!(
        std::fs::read(dir.join("contrat.pdf")).unwrap(),
        b"%PDF-1 first"
    );
    assert_eq!(
        std::fs::read(dir.join("contrat-1.pdf")).unwrap(),
        b"%PDF-1 second"
    );

    // The import neither starts a turn nor changes the node.
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["nodes"][NODE]["status"], "running", "{run}");
}

#[tokio::test]
async fn an_import_over_the_budget_is_a_413_naming_the_setting_and_writes_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "max_attachments_mb": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, NODE, "running").await;

    // Two files: the first fits, the second takes the import over 1 MB. The
    // budget is per import, so neither is written.
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[
            ("small.txt", vec![b'x'; 600 * 1024]),
            ("big.pdf", vec![b'y'; 600 * 1024]),
        ],
    )
    .await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(body["error"], "attachments_too_large");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("max_attachments_mb") && message.contains("big.pdf"),
        "{message}"
    );
    assert!(
        !attachments_dir(&daemon, &run_id, NODE).exists(),
        "nothing written on a refusal"
    );

    // Under the budget, per import: each half lands on its own.
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[("small.txt", vec![b'x'; 600 * 1024])],
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[("big.pdf", vec![b'y'; 600 * 1024])],
    )
    .await;
    assert_eq!(status, 201, "{body}");
}

#[tokio::test]
async fn a_node_without_a_live_session_refuses_the_import_and_writes_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, NODE, "running").await;

    // A node never started: no session.
    let (status, body) = import(&daemon, &run_id, "end", &[("a.pdf", b"x".to_vec())]).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("nothing imported"));
    assert!(!attachments_dir(&daemon, &run_id, "end").exists());

    // A node whose tmux session died before the sweep noticed.
    let session = format!("pdo-{run_id}-{NODE}-iter-1");
    tmux(&daemon, &["kill-session", "-t", &session]);
    let (status, body) = import(&daemon, &run_id, NODE, &[("a.pdf", b"x".to_vec())]).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());

    // An unknown node is a 404.
    let (status, _) = import(&daemon, &run_id, "ghost", &[("a.pdf", b"x".to_vec())]).await;
    assert_eq!(status, 404);

    // The text gesture has the same precondition.
    let (status, body) = send_text(&daemon, &run_id, NODE, "hello").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
}

#[tokio::test]
async fn a_name_that_is_no_file_name_is_refused() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, NODE, "running").await;

    let (status, body) = import(&daemon, &run_id, NODE, &[("..", b"x".to_vec())]).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "invalid_filename");
    // A traversal attempt keeps its last component only.
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[("../../escape.txt", b"x".to_vec())],
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        body["files"][0]["path"],
        format!(".pdo/artifacts/_attachments/{NODE}/escape.txt")
    );
}

#[tokio::test]
async fn the_text_is_written_into_the_session_input_without_enter() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, NODE, "running").await;
    let session = format!("pdo-{run_id}-{NODE}-iter-1");

    let text = "I imported a file for you: `.pdo/artifacts/_attachments/worker/contrat.pdf`";
    let (status, body) = send_text(&daemon, &run_id, NODE, text).await;
    assert_eq!(status, 200, "{body}");

    // The pane (a `sleep`, whose tty echoes input) shows the text, and the
    // cursor sits right after it on the same line: no Enter was sent.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut pane = String::new();
    while tokio::time::Instant::now() < deadline {
        pane = tmux(&daemon, &["capture-pane", "-p", "-t", &session]);
        if pane.contains("contrat.pdf") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(pane.contains("contrat.pdf`"), "text missing: {pane:?}");
    let cursor = tmux(
        &daemon,
        &["display", "-p", "-t", &session, "#{cursor_x} #{cursor_y}"],
    );
    let mut parts = cursor.split_whitespace();
    let x: usize = parts.next().unwrap().parse().unwrap();
    let y: usize = parts.next().unwrap().parse().unwrap();
    let line = pane.lines().nth(y).unwrap_or_default();
    assert!(
        x > 0 && line.trim_end().ends_with("contrat.pdf`"),
        "the cursor must stay on the pasted line (no Enter): x={x} line={line:?} pane={pane:?}"
    );

    // Empty text is refused.
    let (status, _) = send_text(&daemon, &run_id, NODE, "   ").await;
    assert_eq!(status, 400);
}
