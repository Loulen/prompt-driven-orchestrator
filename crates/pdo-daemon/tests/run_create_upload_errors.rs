//! #839 — a `POST /runs` upload that fails between the client and the daemon
//! must read as what it is. Layer 3, against a real daemon:
//!
//! - the daemon's own 413 carries a JSON `error` naming `max_attachments_mb`
//!   (none existed on `POST /runs` before this file);
//! - a multipart body cut mid-stream is a 400 that names the interruption and
//!   the attachment being read — never "missing field: input".

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "upload-errors-test";
const PIPELINE_YAML: &str = r#"name: upload-errors-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: only
    name: only
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: only, port: task }
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
    std::fs::write(prompts_dir.join("only.md"), "You are a worker.\n")?;

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

const BOUNDARY: &str = "pdo839boundary";

/// A `multipart/form-data` body by hand (the test profile's `reqwest` has no
/// `multipart` feature): text fields, then one `files` part per attachment.
fn build_multipart(fields: &[(&str, &str)], files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    for (filename, data) in files {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"files\"; filename=\"{filename}\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: text/html\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn content_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

/// Send `POST /runs` over a raw socket, announcing `announced_len` bytes of
/// body but writing only `sent` of them, then half-close the write side — the
/// shape of a connection cut mid-upload. Returns the status line and body.
async fn post_truncated(daemon: &TestDaemon, sent: &[u8], announced_len: usize) -> (u16, String) {
    let mut sock = tokio::net::TcpStream::connect(daemon.addr).await.unwrap();
    let head = format!(
        "POST /runs HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        daemon.addr,
        content_type(),
        announced_len
    );
    sock.write_all(head.as_bytes()).await.unwrap();
    sock.write_all(sent).await.unwrap();
    sock.shutdown().await.unwrap();

    let mut raw = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sock.read_to_end(&mut raw),
    )
    .await
    .expect("the daemon must answer a truncated upload, not hang")
    .unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in: {text:?}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[tokio::test]
async fn attachments_over_the_budget_are_a_413_naming_max_attachments_mb() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "max_attachments_mb": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the budget must be settable to 1 MB");

    let big = vec![b'x'; 1024 * 1024 + 512 * 1024]; // 1.5 MB > 1 MB
    let target_repo = daemon.target_repo();
    let body = build_multipart(
        &[
            ("pipeline", PIPELINE_NAME),
            ("input", "with a big file"),
            ("target_repo", target_repo.as_str()),
        ],
        &[("copilote.html", &big)],
    );
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .header("content-type", content_type())
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
    let json: serde_json::Value = resp
        .json()
        .await
        .expect("the daemon's 413 carries a JSON body — what tells it apart from a proxy's");
    let error = json["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("max_attachments_mb") && error.contains("copilote.html"),
        "the daemon's 413 names the budget and the file: {error}"
    );
}

#[tokio::test]
async fn a_body_cut_at_a_field_boundary_is_an_interrupted_upload_not_a_missing_field() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let target_repo = daemon.target_repo();
    let full = build_multipart(
        &[
            ("pipeline", PIPELINE_NAME),
            ("input", "hello"),
            ("target_repo", target_repo.as_str()),
        ],
        &[("copilote.html", &vec![b'y'; 64 * 1024])],
    );
    // Cut right after the `pipeline` field: the body ends at a field boundary,
    // the one shape that used to read as "missing field: input".
    let cut_at = {
        let marker = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"input\"");
        full.windows(marker.len())
            .position(|w| w == marker.as_bytes())
            .expect("input field present")
    };
    let (status, body) = post_truncated(&daemon, &full[..cut_at], full.len()).await;

    assert_eq!(status, 400, "body: {body}");
    assert!(
        !body.contains("missing field"),
        "a cut upload must not read as a missing field: {body}"
    );
    assert!(
        body.contains("upload interrupted") && body.contains("after field `pipeline`"),
        "the 400 names the interruption and where it happened: {body}"
    );
}

#[tokio::test]
async fn a_body_cut_inside_a_file_part_names_the_attachment() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let target_repo = daemon.target_repo();
    let full = build_multipart(
        &[
            ("pipeline", PIPELINE_NAME),
            ("input", "hello"),
            ("target_repo", target_repo.as_str()),
        ],
        &[("copilote.html", &vec![b'y'; 256 * 1024])],
    );
    // Cut halfway through the file bytes.
    let cut_at = full.len() - 100 * 1024;
    let (status, body) = post_truncated(&daemon, &full[..cut_at], full.len()).await;

    assert_eq!(status, 400, "body: {body}");
    assert!(
        body.contains("upload interrupted") && body.contains("copilote.html"),
        "the 400 names the attachment being read when the cut happened: {body}"
    );
    assert!(
        body.contains("client_max_body_size") && body.contains("max_attachments_mb"),
        "the 400 points at the proxy body limit and PDO's own budget: {body}"
    );
}
