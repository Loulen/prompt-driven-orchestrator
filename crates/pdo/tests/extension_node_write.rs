//! API 0.23 — **an extension answers a Node that waits for a person**, on a
//! real daemon with the test extension and a real tmux: `Host::write_to_node`
//! types the text into the Node's session and submits it, the Node resumes,
//! the Actor is announced as taking control; a Node that no longer waits, or
//! an Actor who may not take control, is refused and nothing is written.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::common::flow::{seed_solo, wait_node_status};
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// A daemon with the test extension whose every session appends each line
/// it reads (each Enter) to `<lines>/<session name>.lines`.
async fn daemon(lines: &Path) -> (TestDaemon, Control) {
    let tail = format!(
        "f='{}'/\"$(tmux display-message -p '#S')\".lines; : > \"$f\"; \
         while IFS= read -r line; do printf '%s\\n' \"$line\" >> \"$f\"; done; exec sleep 600",
        lines.display()
    );
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(
        |repo| seed_solo(repo, &[("solo", false)]),
        move |root, config| {
            config.tmux_cmd_override = Some(tail);
            config.sandbox_home_override = Some(root.join("home"));
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap();
    (daemon, control)
}

/// `method path` as `actor` (the test extension's header).
async fn call_as(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    actor: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{}{path}", daemon.url()))
        .header(ACTOR_HEADER, actor);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// `alice` creates a Run of `solo` whose `worker` waits for a person; its id.
async fn waiting_run_of_alice(daemon: &TestDaemon, lines: &Path) -> String {
    let (status, body) = call_as(
        daemon,
        reqwest::Method::POST,
        "/runs",
        "alice",
        Some(json!({
            "pipeline": "solo",
            "input": "go",
            "name": "solo run",
            "target_repo": daemon.target_repo(),
        })),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();
    wait_node_status(daemon, &run_id, "worker", "running").await;
    // The pane reads its input before anyone writes into it.
    lines_of(lines, &session(&run_id)).await;
    let (status, body) = call_as(
        daemon,
        reqwest::Method::POST,
        &format!("/runs/{run_id}/nodes/worker/wait-user"),
        "alice",
        Some(json!({ "iter": 1, "message": "Which retention?" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    wait_node_status(daemon, &run_id, "worker", "awaiting_user").await;
    run_id
}

fn session(run_id: &str) -> String {
    format!("pdo-{run_id}-worker-iter-1")
}

/// The lines the session `name` read so far, once its file exists.
async fn lines_of(lines: &Path, name: &str) -> Vec<String> {
    let file: PathBuf = lines.join(format!("{name}.lines"));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(&file) {
            return text.lines().map(str::to_string).collect();
        }
        assert!(
            Instant::now() < deadline,
            "{} never written",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The lines of `name` once there are `count`.
async fn lines_until(lines: &Path, name: &str, count: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let read = lines_of(lines, name).await;
        if read.len() >= count {
            return read;
        }
        assert!(Instant::now() < deadline, "only {read:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn write_as(daemon: &TestDaemon, actor: &str, run_id: &str, text: &str) -> (u16, Value) {
    call_as(
        daemon,
        reqwest::Method::POST,
        &format!("/ext/test/runs/{run_id}/nodes/worker/write"),
        actor,
        Some(json!({ "text": text })),
    )
    .await
}

/// FP 2 and 3: Bob answers the Node of Alice's Run through the extension; the
/// session reads « continue », the Node runs again, Bob takes control; once
/// the Node runs, a second answer is refused and nothing more is read.
#[tokio::test]
async fn bob_answers_a_waiting_node_and_a_second_answer_is_refused() {
    let lines = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(lines.path()).await;
    let run_id = waiting_run_of_alice(&daemon, lines.path()).await;
    let name = session(&run_id);

    let (status, body) = write_as(&daemon, "bob", &run_id, "continue").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["written"]["iter"], 1);
    assert_eq!(body["written"]["session"], name.as_str());
    assert_eq!(
        body["link"],
        format!("#/runs/{run_id}/nodes/worker/iter/1").as_str()
    );
    assert_eq!(lines_until(lines.path(), &name, 1).await, ["continue"]);
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let taken = control
            .events()
            .into_iter()
            .find(|e| e["type"] == "session_taken_over" && e["session"]["id"] == name.as_str());
        if let Some(taken) = taken {
            assert_eq!(
                taken["actor"],
                json!({ "kind": "identified", "namespace": "test", "id": "bob" })
            );
            break;
        }
        assert!(Instant::now() < deadline, "{:#?}", control.events());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let (status, body) = write_as(&daemon, "bob", &run_id, "again").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["reason"], "not_waiting");
    assert_eq!(body["error"], "the node no longer waits");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(lines_of(lines.path(), &name).await, ["continue"]);

    // A Node the Run does not hold, nor a Run the daemon knows.
    let (status, body) = call_as(
        &daemon,
        reqwest::Method::POST,
        &format!("/ext/test/runs/{run_id}/nodes/ghost/write"),
        "bob",
        Some(json!({ "text": "hello" })),
    )
    .await;
    assert_eq!((status, &body["reason"]), (404, &json!("not_waiting")));
}

/// An Actor who may not take control of the Node's terminal is refused while
/// the Node waits, and nothing is written; the Node still waits.
#[tokio::test]
async fn an_actor_who_may_not_take_control_writes_nothing() {
    let lines = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(lines.path()).await;
    let run_id = waiting_run_of_alice(&daemon, lines.path()).await;
    control.set(Refusal::parse("session:take_control~bob").unwrap());

    let (status, body) = write_as(&daemon, "bob", &run_id, "continue").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["reason"], "refused");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(lines_of(lines.path(), &session(&run_id)).await.is_empty());
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["nodes"]["worker"]["status"], "awaiting_user");
    assert!(!control
        .events()
        .iter()
        .any(|e| e["type"] == "session_taken_over" && e["actor"]["id"] == "bob"));

    // Carol may: her answer lands.
    let (status, body) = write_as(&daemon, "carol", &run_id, "go on").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        lines_until(lines.path(), &session(&run_id), 1).await,
        ["go on"]
    );
}
