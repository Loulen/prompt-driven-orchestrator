//! « Fichier importé en cours de Run », over a real daemon:
//!
//! - files imported into a node with a live session land in the Run's
//!   Blackboard under `.pdo/artifacts/_attachments/<node-id>/`, and the answer
//!   names each final path relative to the worktree root; a name already there
//!   is suffixed (`contrat-1.pdf`), never overwritten; the files stay out of
//!   `git status`;
//! - every named refusal (404, 409, 413, 400) writes nothing, and a write that
//!   fails half-way takes back what it wrote;
//! - « Copy & send to terminal » writes the text into the session's input
//!   **without** Enter, two concurrent texts never mix, and the Enter the user
//!   then types lifts a declared wait;
//! - a poste that watches the node's terminal as a spectator is refused the
//!   text (`409 not_pilot`, the pilot named), the pilot and a request from no
//!   poste are not;
//! - an extension refusing `Node × import_files` or `Node × send_text` gets
//!   its refusal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pdo::extension_api::{ResourceKind, Verb};
use pdo_session::domain::names::node_session_name;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::common::flow::{git, git_init};
use crate::common::TestDaemon;
use crate::declared_wait::{create_run, get_json, wait_node_status, wait_user};
use crate::test_extension::{Refusal, TestExtension};

const IMPORT: &str = "import-pipe";
const SCRIPTED: &str = "import-script";
const NODE: &str = "worker";

const IMPORT_YAML: &str = r#"name: import-pipe
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
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
"#;

/// `start → scripter (script, sleeps) → end`.
const SCRIPTED_YAML: &str = r#"name: import-script
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: scripter
    name: scripter
    type: script
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: scripter, port: in }
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

fn seed(repo: &Path) -> anyhow::Result<()> {
    let dir = repo.join(".pdo/pipelines");
    std::fs::create_dir_all(dir.join(format!("{IMPORT}.prompts")))?;
    std::fs::create_dir_all(dir.join(format!("{SCRIPTED}.prompts")))?;
    std::fs::write(dir.join(format!("{IMPORT}.yaml")), IMPORT_YAML)?;
    std::fs::write(
        dir.join(format!("{IMPORT}.prompts/worker.md")),
        "Read what the user imports.\n",
    )?;
    std::fs::write(dir.join(format!("{SCRIPTED}.yaml")), SCRIPTED_YAML)?;
    std::fs::write(
        dir.join(format!("{SCRIPTED}.prompts/scripter.md")),
        "#!/usr/bin/env bash\nsleep 600\n",
    )?;
    git_init(repo)
}

fn worktree(daemon: &TestDaemon, run_id: &str) -> PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
}

fn attachments_dir(daemon: &TestDaemon, run_id: &str, node: &str) -> PathBuf {
    worktree(daemon, run_id)
        .join(".pdo/artifacts/_attachments")
        .join(node)
}

async fn import(
    daemon: &TestDaemon,
    run_id: &str,
    node: &str,
    files: &[(&str, Vec<u8>)],
) -> (u16, Value) {
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
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn send_text(daemon: &TestDaemon, run_id: &str, node: &str, text: &str) -> (u16, Value) {
    send_text_from(daemon, run_id, node, text, None).await
}

async fn send_text_from(
    daemon: &TestDaemon,
    run_id: &str,
    node: &str,
    text: &str,
    poste: Option<&str>,
) -> (u16, Value) {
    let mut body = json!({ "text": text });
    if let Some(poste) = poste {
        body["poste"] = json!(poste);
    }
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node}/terminal-text",
            daemon.url()
        ))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
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

/// Wait until the pane shows every needle; return the pane.
async fn pane_with(daemon: &TestDaemon, session: &str, needles: &[&str]) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut pane = String::new();
    while tokio::time::Instant::now() < deadline {
        pane = tmux(daemon, &["capture-pane", "-p", "-J", "-t", session]);
        if needles.iter().all(|n| pane.contains(n)) {
            return pane;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the pane never showed {needles:?}: {pane:?}");
}

async fn running_import_run(daemon: &TestDaemon) -> String {
    let run_id = create_run(daemon, IMPORT, None).await;
    wait_node_status(daemon, &run_id, NODE, "running").await;
    run_id
}

#[tokio::test]
async fn imported_files_land_in_the_blackboard_and_a_taken_name_is_suffixed() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;

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
        body,
        json!({
            "iter": 1,
            "files": [
                {
                    "name": "contrat.pdf",
                    "path": format!(".pdo/artifacts/_attachments/{NODE}/contrat.pdf"),
                    "size": 12,
                },
                {
                    "name": "annexe.csv",
                    "path": format!(".pdo/artifacts/_attachments/{NODE}/annexe.csv"),
                    "size": 4,
                },
            ],
        })
    );
    // The returned path resolves from the worktree root.
    let wt = worktree(&daemon, &run_id);
    let path = body["files"][0]["path"].as_str().unwrap();
    assert_eq!(std::fs::read(wt.join(path)).unwrap(), b"%PDF-1 first");

    // Same name again, twice in one import: suffixed, the first one untouched.
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[
            ("contrat.pdf", b"%PDF-1 second".to_vec()),
            ("contrat.pdf", b"%PDF-1 third".to_vec()),
        ],
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["files"][0]["name"], "contrat-1.pdf");
    assert_eq!(body["files"][1]["name"], "contrat-2.pdf");
    let dir = attachments_dir(&daemon, &run_id, NODE);
    assert_eq!(
        std::fs::read(dir.join("contrat.pdf")).unwrap(),
        b"%PDF-1 first"
    );
    assert_eq!(
        std::fs::read(dir.join("contrat-1.pdf")).unwrap(),
        b"%PDF-1 second"
    );
    assert_eq!(
        std::fs::read(dir.join("contrat-2.pdf")).unwrap(),
        b"%PDF-1 third"
    );

    // Never committed: the Blackboard is out of the Run's branch.
    let status = git(&wt, &["status", "--porcelain", "--untracked-files=all"]);
    assert!(!status.contains("_attachments"), "{status}");

    // The import neither starts a turn nor changes the node.
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["nodes"][NODE]["status"], "running", "{run}");
}

#[tokio::test]
async fn an_import_over_the_budget_is_a_413_naming_the_setting_and_writes_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&json!({ "max_attachments_mb": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run_id = running_import_run(&daemon).await;

    // The first file fits, the second takes the import over 1 MB: the budget
    // is per import, so neither is written.
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
        message.contains("max_attachments_mb")
            && message.contains("big.pdf")
            && message.ends_with("nothing imported")
            // The upload is cut at the line: no partial amount to mislead.
            && !message.contains("received"),
        "{message}"
    );
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());

    // Per import: each half lands on its own.
    for (name, byte) in [("small.txt", b'x'), ("big.pdf", b'y')] {
        let (status, body) =
            import(&daemon, &run_id, NODE, &[(name, vec![byte; 600 * 1024])]).await;
        assert_eq!(status, 201, "{body}");
    }
}

#[tokio::test]
async fn a_node_without_a_live_session_refuses_by_name_and_writes_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let file = [("a.pdf", b"x".to_vec())];

    // A node never started: no session.
    let (status, body) = import(&daemon, &run_id, "end", &file).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .ends_with("nothing imported"));
    assert!(!attachments_dir(&daemon, &run_id, "end").exists());

    // Unknown node, unknown Run: 404.
    let (status, body) = import(&daemon, &run_id, "ghost", &file).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("node_not_found"))
    );
    let (status, body) = import(&daemon, "20990101-000000-0000000", NODE, &file).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("run_not_found"))
    );

    // The worktree is gone from disk.
    let wt = worktree(&daemon, &run_id);
    let parked = wt.with_file_name("worktree-parked");
    std::fs::rename(&wt, &parked).unwrap();
    let (status, body) = import(&daemon, &run_id, NODE, &file).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "run_worktree_missing");
    assert!(!wt.exists());
    std::fs::rename(&parked, &wt).unwrap();

    // A node whose tmux session died before the sweep noticed.
    let session = node_session_name(&run_id, NODE, 1);
    tmux(&daemon, &["kill-session", "-t", &session]);
    let (status, body) = import(&daemon, &run_id, NODE, &file).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());

    // The text gesture has the same precondition.
    let (status, body) = send_text(&daemon, &run_id, NODE, "hello").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .ends_with("nothing written"));
}

#[tokio::test]
async fn a_script_node_and_a_finished_run_refuse_the_import() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let file = [("a.pdf", b"x".to_vec())];

    let scripted = create_run(&daemon, SCRIPTED, None).await;
    wait_node_status(&daemon, &scripted, "scripter", "running").await;
    let (status, body) = import(&daemon, &scripted, "scripter", &file).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_is_script");
    assert!(!attachments_dir(&daemon, &scripted, "scripter").exists());
    let (status, body) = send_text(&daemon, &scripted, "scripter", "hello").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("node_is_script"))
    );

    // The worker completes, the Run with it: nothing is live any more.
    let run_id = running_import_run(&daemon).await;
    let port = worktree(&daemon, &run_id).join(".pdo/artifacts/worker/iter-1/result");
    std::fs::create_dir_all(&port).unwrap();
    std::fs::write(port.join("output.md"), "done\n").unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/{NODE}/done", daemon.url()))
        .json(&json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    crate::common::flow::wait_run(&daemon, &run_id, "completed", |run| {
        run["status"] == "completed"
    })
    .await;
    let (status, body) = import(&daemon, &run_id, NODE, &file).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "run_not_live");
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());
}

#[tokio::test]
async fn a_malformed_import_is_a_400_and_writes_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let url = format!("{}/runs/{run_id}/nodes/{NODE}/attachments", daemon.url());
    let client = reqwest::Client::new();

    // No name a file may take; the prompt's reserved name.
    for name in ["..", "output.md"] {
        let (status, body) = import(&daemon, &run_id, NODE, &[(name, b"x".to_vec())]).await;
        assert_eq!(status, 400, "{name}: {body}");
        assert_eq!(body["error"], "invalid_filename");
    }
    // A field other than `files`.
    let form = reqwest::multipart::Form::new().text("note", "hi");
    let resp = client.post(&url).multipart(form).send().await.unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "unknown_field");
    // No file at all: a multipart body with its closing boundary only.
    let resp = client
        .post(&url)
        .header("content-type", "multipart/form-data; boundary=X")
        .body("--X--\r\n")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "no_files");
    // Not multipart.
    let resp = client.post(&url).json(&json!({})).send().await.unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "bad_multipart");
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());

    // A traversal keeps its last component only.
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
async fn a_write_that_fails_half_way_takes_back_what_it_wrote() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    // The second name is longer than any file system allows.
    let too_long = format!("{}.txt", "n".repeat(300));
    let (status, body) = import(
        &daemon,
        &run_id,
        NODE,
        &[("first.txt", b"1".to_vec()), (&too_long, b"2".to_vec())],
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"], "write_failed");
    assert!(
        !worktree(&daemon, &run_id)
            .join(".pdo/artifacts/_attachments")
            .exists(),
        "the folders this import created are gone too"
    );
}

#[tokio::test]
async fn the_text_is_written_into_the_session_input_without_enter() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let session = node_session_name(&run_id, NODE, 1);

    let text = "I imported a file for you: `.pdo/artifacts/_attachments/worker/contrat.pdf`";
    let (status, body) = send_text(&daemon, &run_id, NODE, text).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({ "ok": true, "iter": 1 }));

    // The pane (a `sleep`, whose tty echoes input) shows the text, and the
    // cursor sits right after it on the same line: no Enter was sent.
    let pane = pane_with(&daemon, &session, &["contrat.pdf`"]).await;
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

    // Empty or too long: 400, nothing written.
    let (status, body) = send_text(&daemon, &run_id, NODE, "   ").await;
    assert_eq!((status, body["error"].as_str()), (400, Some("empty_text")));
    let (status, body) = send_text(&daemon, &run_id, NODE, &"x".repeat(16 * 1024 + 1)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("text_too_long"))
    );
    let (status, body) = send_text(&daemon, &run_id, "ghost", "hello").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("node_not_found"))
    );
}

#[tokio::test]
async fn two_concurrent_texts_never_mix() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let session = node_session_name(&run_id, NODE, 1);
    let first = format!("first-{}", "a".repeat(400));
    let second = format!("second-{}", "b".repeat(400));
    let (a, b) = tokio::join!(
        send_text(&daemon, &run_id, NODE, &first),
        send_text(&daemon, &run_id, NODE, &second),
    );
    assert_eq!((a.0, b.0), (200, 200), "{a:?} {b:?}");
    // Each paste arrives whole: one text never takes the other's buffer.
    let pane = pane_with(&daemon, &session, &["first-", "second-"]).await;
    let joined: String = pane.split_whitespace().collect();
    assert!(
        joined.contains(&first) && joined.contains(&second),
        "{pane:?}"
    );
}

#[tokio::test]
async fn the_enter_typed_after_the_text_lifts_the_declared_wait() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let (status, body) = wait_user(&daemon, &run_id, NODE, Some("Which contract?")).await;
    assert_eq!(status, 200, "{body}");
    wait_node_status(&daemon, &run_id, NODE, "awaiting_user").await;

    let (status, body) = send_text(&daemon, &run_id, NODE, "I imported a file for you").await;
    assert_eq!(status, 200, "{body}");
    // Writing the text does not lift the wait…
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["nodes"][NODE]["status"], "awaiting_user", "{run}");

    // …the user's Enter in the terminal does, no other byte typed.
    let session = node_session_name(&run_id, NODE, 1);
    let url = format!("ws://{}/sessions/{session}/pty", daemon.addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws.send(Message::Binary(b"\r".to_vec().into()))
        .await
        .unwrap();
    wait_node_status(&daemon, &run_id, NODE, "running").await;
    let _ = ws.close(None).await;
}

#[tokio::test]
async fn an_extension_refusing_the_verbs_refuses_the_gestures() {
    let (ext, control) = TestExtension::new(Refusal::Only(
        ResourceKind::Node,
        Verb::Action("import_files".into()),
    ));
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let run_id = running_import_run(&daemon).await;

    let (status, body) = import(&daemon, &run_id, NODE, &[("a.pdf", b"x".to_vec())]).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    assert!(!attachments_dir(&daemon, &run_id, NODE).exists());
    // The other verb is its own: the text still goes.
    let (status, body) = send_text(&daemon, &run_id, NODE, "hello").await;
    assert_eq!(status, 200, "{body}");

    control.set(Refusal::Only(
        ResourceKind::Node,
        Verb::Action("send_text".into()),
    ));
    let (status, body) = send_text(&daemon, &run_id, NODE, "hello").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    let (status, body) = import(&daemon, &run_id, NODE, &[("a.pdf", b"x".to_vec())]).await;
    assert_eq!(status, 201, "{body}");
}

type Terminal =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The terminal of `session`, opened by `poste`.
async fn open_terminal(daemon: &TestDaemon, session: &str, poste: &str) -> Terminal {
    let url = format!("ws://{}/sessions/{session}/pty?poste={poste}", daemon.addr);
    tokio_tungstenite::connect_async(&url)
        .await
        .expect("the terminal socket opens")
        .0
}

/// Read the terminal's frames until its role is `role`.
async fn wait_role(terminal: &mut Terminal, role: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, terminal.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let frame: Value = serde_json::from_str(&text).unwrap();
                if frame["type"] == "role" && frame["role"] == role {
                    return;
                }
            }
            Ok(Some(Ok(_))) => {}
            other => panic!("the terminal never became {role}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_spectator_poste_is_refused_the_text_and_the_pilot_is_not() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = running_import_run(&daemon).await;
    let session = node_session_name(&run_id, NODE, 1);
    let mut a = open_terminal(&daemon, &session, "posteA").await;
    wait_role(&mut a, "solo").await;
    let mut b = open_terminal(&daemon, &session, "posteB").await;
    wait_role(&mut b, "spectator").await;
    wait_role(&mut a, "pilot").await;

    let (status, body) =
        send_text_from(&daemon, &run_id, NODE, "spectator-text", Some("posteB")).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "not_pilot");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("another poste has the terminal"),
        "{body}"
    );
    assert_eq!(body["pilot"]["kind"], "local_operator", "{body}");

    // The pilot's text, then one from no poste, reach the input; the
    // spectator's never does.
    let (status, body) = send_text_from(&daemon, &run_id, NODE, "pilot-text", Some("posteA")).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = send_text(&daemon, &run_id, NODE, "cli-text").await;
    assert_eq!(status, 200, "{body}");
    let pane = pane_with(&daemon, &session, &["pilot-text", "cli-text"]).await;
    assert!(!pane.contains("spectator-text"), "{pane:?}");

    // Once it takes control, the former spectator writes.
    b.send(Message::Text(r#"{"type":"take_control"}"#.into()))
        .await
        .unwrap();
    wait_role(&mut b, "pilot").await;
    let (status, body) = send_text_from(&daemon, &run_id, NODE, "taken-text", Some("posteB")).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = send_text_from(&daemon, &run_id, NODE, "late-text", Some("posteA")).await;
    assert_eq!(status, 409, "the former pilot now watches: {body}");
    pane_with(&daemon, &session, &["taken-text"]).await;
}
