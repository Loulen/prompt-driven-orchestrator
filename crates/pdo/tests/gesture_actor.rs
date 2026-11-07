//! The Actor of the gestures on a Run, on a real daemon with the test
//! extension (two Actors, `alice` and `bob`) and a real tmux: each event of a
//! gesture names who made it (`payload.actor`), taking the hand on a node's
//! terminal is an event of its own, typing makes one event per turn of
//! speech, and `pdo run events` shows who made each event by name.

use std::process::Command;
use std::time::{Duration, Instant};

use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::flow::{seed_solo, wait_node_status};
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension, ACTOR_HEADER};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A daemon with the test extension, whose `solo` pipeline has an interactive
/// `worker` node held by a session that reads its input line by line.
async fn daemon() -> TestDaemon {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    TestDaemon::spawn_configured(
        |repo| seed_solo(repo, &[("solo", true)]),
        move |root, config| {
            config.tmux_cmd_override =
                Some("while IFS= read -r line; do :; done; exec sleep 600".into());
            config.sandbox_home_override = Some(root.join("home"));
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap()
}

/// `method path` as `actor`.
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

async fn post_as(daemon: &TestDaemon, path: &str, actor: &str, body: Value) -> Value {
    let (status, answer) = call_as(daemon, reqwest::Method::POST, path, actor, Some(body)).await;
    assert!(
        (200..300).contains(&status),
        "POST {path}: {status} {answer}"
    );
    answer
}

/// `alice` creates a Run of `solo`; its id, once `worker` runs.
async fn run_of_alice(daemon: &TestDaemon) -> String {
    let body = post_as(
        daemon,
        "/runs",
        "alice",
        json!({
            "pipeline": "solo",
            "input": "go",
            "name": "solo run",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    let run_id = body["run_id"].as_str().unwrap().to_string();
    wait_node_status(daemon, &run_id, "worker", "running").await;
    run_id
}

fn session(run_id: &str, iter: i64) -> String {
    pdo_session::domain::names::node_session_name(run_id, "worker", iter)
}

/// The terminal of `session`, opened by `actor` from the poste `poste`.
async fn terminal_of(daemon: &TestDaemon, session: &str, actor: &str, poste: &str) -> Ws {
    let url = format!("ws://{}/sessions/{session}/pty?poste={poste}", daemon.addr);
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert(ACTOR_HEADER, actor.parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"resize","cols":100,"rows":30}"#.into(),
    ))
    .await
    .unwrap();
    ws
}

/// Type `text` one keystroke per frame.
async fn type_keys(ws: &mut Ws, text: &str) {
    for key in text.bytes() {
        ws.send(Message::Binary(vec![key].into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn events(daemon: &TestDaemon, run_id: &str) -> Vec<Value> {
    daemon
        .get_json(&format!("/runs/{run_id}/events"))
        .await
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The events of `kind`, once `count` of them are in the log.
async fn events_of(daemon: &TestDaemon, run_id: &str, kind: &str, count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found: Vec<Value> = events(daemon, run_id)
            .await
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .collect();
        if found.len() >= count {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "only {} {kind}: {:#?}",
            found.len(),
            events(daemon, run_id).await
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn of(event: &Value) -> &str {
    event["payload"]["actor"]["id"].as_str().unwrap_or("")
}

fn kinds_by(events: &[Value], kind: &str) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["kind"] == kind)
        .map(|e| of(e).to_string())
        .collect()
}

/// `pdo run events <run>` against `daemon`.
async fn pdo_run_events(daemon: &TestDaemon, run_id: &str) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
    cmd.args(["run", "events", run_id]);
    cmd.env("PDO_DAEMON_URL", daemon.url());
    cmd.env_remove("PDO_RUN_ID");
    cmd.env_remove("PDO_NODE_ID");
    cmd.env_remove("PDO_TOKEN");
    let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The FP: Bob takes the hand on the terminal of Alice's waiting node and
/// answers; Alice releases the completion, stops, retries, then completes the
/// node in its agent's place. Each event names who made it; Bob's answer is
/// one line, whatever the number of keystrokes.
#[tokio::test]
async fn each_gesture_on_a_run_names_who_made_it() {
    let daemon = daemon().await;
    let run_id = run_of_alice(&daemon).await;
    post_as(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/wait-user"),
        "alice",
        json!({ "iter": 1, "message": "Which retention?" }),
    )
    .await;
    wait_node_status(&daemon, &run_id, "worker", "awaiting_user").await;

    let mut bob = terminal_of(&daemon, &session(&run_id, 1), "bob", "posteB").await;
    events_of(&daemon, &run_id, "node_taken_over", 1).await;
    type_keys(&mut bob, "thirty day").await;
    type_keys(&mut bob, "\r").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    // The node runs again: Bob's next keystrokes are the same turn.
    type_keys(&mut bob, "more").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let _ = bob.close(None).await;

    post_as(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/release"),
        "alice",
        json!({ "iter": 1 }),
    )
    .await;
    post_as(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/stop"),
        "alice",
        json!({}),
    )
    .await;
    post_as(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/retry"),
        "alice",
        json!({}),
    )
    .await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    post_as(
        &daemon,
        &format!("/runs/{run_id}/nodes/worker/skip"),
        "alice",
        json!({ "iter": 2, "reason": "nothing left" }),
    )
    .await;
    events_of(&daemon, &run_id, "run_skipped", 1).await;

    let log = events(&daemon, &run_id).await;
    assert_eq!(kinds_by(&log, "node_taken_over"), ["bob"], "{log:#?}");
    assert_eq!(kinds_by(&log, "node_resumed"), ["bob"], "{log:#?}");
    assert!(
        kinds_by(&log, "node_typed").is_empty(),
        "the answer is the resumed line: {log:#?}"
    );
    assert_eq!(kinds_by(&log, "node_completion_released"), ["alice"]);
    assert_eq!(kinds_by(&log, "node_stopped"), ["alice"]);
    let restarted: Vec<&Value> = log
        .iter()
        .filter(|e| e["kind"] == "node_started" && e["iter"] == 2)
        .collect();
    assert_eq!(restarted.len(), 1, "{log:#?}");
    assert_eq!(of(restarted[0]), "alice", "the retry's start: {log:#?}");
    let skipped: Vec<&Value> = log
        .iter()
        .filter(|e| e["kind"] == "node_completed" && e["iter"] == 2)
        .collect();
    assert_eq!(skipped.len(), 1, "{log:#?}");
    assert_eq!(of(skipped[0]), "alice");
    // Nobody made the Run's own transitions.
    let started = log.iter().find(|e| e["kind"] == "node_started").unwrap();
    assert_eq!(started["iter"], 1);
    assert!(started["payload"].get("actor").is_none(), "{started}");

    let printed = pdo_run_events(&daemon, &run_id).await;
    let who = |kind: &str| -> Vec<String> {
        printed
            .lines()
            .filter(|line| line.split('\t').nth(1) == Some(kind))
            .map(|line| line.rsplit('\t').next().unwrap().to_string())
            .collect()
    };
    assert_eq!(who("node_taken_over"), ["Bob"], "{printed}");
    assert_eq!(who("node_resumed"), ["Bob"], "{printed}");
    assert_eq!(who("node_completion_released"), ["Alice"], "{printed}");
    assert_eq!(who("node_stopped"), ["Alice"], "{printed}");
    assert_eq!(who("run_started"), [""], "{printed}");
}

/// Alice drives the terminal of her running node, Bob takes control and types
/// ten keystrokes: two take-overs, one line for Bob's turn. Alice marks the
/// node done through the Pipeline Manager's command.
#[tokio::test]
async fn ten_keystrokes_after_a_take_over_make_one_line() {
    let daemon = daemon().await;
    let run_id = run_of_alice(&daemon).await;
    let name = session(&run_id, 1);

    let mut alice = terminal_of(&daemon, &name, "alice", "posteA").await;
    events_of(&daemon, &run_id, "node_taken_over", 1).await;
    let mut bob = terminal_of(&daemon, &name, "bob", "posteB").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    bob.send(Message::Text(r#"{"type":"take_control"}"#.into()))
        .await
        .unwrap();
    events_of(&daemon, &run_id, "node_taken_over", 2).await;
    type_keys(&mut bob, "ten  keys\r").await;
    events_of(&daemon, &run_id, "node_typed", 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let _ = alice.close(None).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    post_as(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        "alice",
        json!({ "kind": "inject_artifact", "path": "worker/iter-1/out/output.md", "content": "done" }),
    )
    .await;
    post_as(
        &daemon,
        &format!("/runs/{run_id}/commands"),
        "alice",
        json!({ "kind": "mark_node_done", "node_id": "worker", "iter": 1 }),
    )
    .await;
    let _ = bob.close(None).await;

    let log = events(&daemon, &run_id).await;
    assert_eq!(
        kinds_by(&log, "node_taken_over"),
        ["alice", "bob"],
        "{log:#?}"
    );
    assert_eq!(kinds_by(&log, "node_typed"), ["bob"], "{log:#?}");
    let done: Vec<&Value> = log
        .iter()
        .filter(|e| e["kind"] == "node_completed" && e["node_id"] == "worker")
        .collect();
    assert_eq!(done.len(), 1, "{log:#?}");
    assert_eq!(of(done[0]), "alice");
    assert_eq!(
        kinds_by(&log, "command_issued"),
        ["alice", "alice"],
        "inject then mark done: {log:#?}"
    );
}
