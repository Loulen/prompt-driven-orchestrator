//! API 0.13 — **the sessions of an agent and the Actor who starts them**
//! (CORE-61), on a real daemon with the test extension and a real tmux: the
//! launch hooks asked for the Pipeline Manager and the library assistant with
//! who starts them, what they give in the session's environment, one assistant
//! per Actor, and the session events (`session_started`,
//! `session_taken_over`, `session_ended`) on `/ws` and in `on_event`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::common::flow::seed_solo;
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A daemon with the test extension whose every session writes its
/// environment to `<envs>/<session name>.env`, then sleeps.
async fn daemon(envs: &Path) -> (TestDaemon, Control) {
    let tail = format!(
        "env > '{}'/\"$(tmux display-message -p '#S')\".env; exec sleep 600",
        envs.display()
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
    control
        .launch
        .set_env([("FIXTURE_HELLO".to_string(), "monde".to_string())].into());
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

/// `alice` creates a Run of `solo`; its id.
async fn run_of_alice(daemon: &TestDaemon) -> String {
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
    body["run_id"].as_str().unwrap().to_string()
}

/// The environment the session `name` was started with, once it wrote it.
async fn env_of(envs: &Path, name: &str) -> String {
    let file: PathBuf = envs.join(format!("{name}.env"));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(&file) {
            if text.contains("PATH=") {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} never written",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Wait until `check` holds on the events the test extension received.
async fn extension_events(control: &Control, check: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let events = control.events();
        if check(&events) {
            return events;
        }
        assert!(Instant::now() < deadline, "never came: {events:#?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn session_event<'a>(events: &'a [Value], kind: &str, session: &str) -> Option<&'a Value> {
    events
        .iter()
        .find(|e| e["type"] == kind && e["session"]["id"] == session)
}

fn launches_of(control: &Control, hook: &str, session: &str) -> Vec<Value> {
    control
        .launch
        .launches()
        .into_iter()
        .filter(|l| l["hook"] == hook && l["launch"]["session"]["id"] == session)
        .map(|l| l["launch"].clone())
        .collect()
}

fn identified(id: &str) -> Value {
    json!({ "kind": "identified", "namespace": "test", "id": id })
}

/// FP 1: Alice creates a Run, Bob starts its manager. The extension was asked
/// for the manager with Bob as who starts it, and what it gives is in the
/// manager's environment; the manager's start, then its end, reach the
/// extension and `/ws` with Bob.
#[tokio::test]
async fn the_manager_bob_starts_on_the_run_of_alice_carries_what_the_extension_gives_bob() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    control.launch.set_preamble(Some("Ligne de fixture".into()));
    let run_id = run_of_alice(&daemon).await;
    let mut ws = daemon.connect_ws_as("alice").await.unwrap();

    let (status, body) = call_as(
        &daemon,
        reqwest::Method::POST,
        &format!("/runs/{run_id}/manager/start"),
        "bob",
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["created"], true);
    let manager = body["session"].as_str().unwrap().to_string();

    let launches = launches_of(&control, "session_env", &manager);
    assert_eq!(launches.len(), 1, "{launches:#?}");
    assert_eq!(launches[0]["started_by"], identified("bob"));
    assert_eq!(launches[0]["session"]["kind"], "manager");
    assert_eq!(launches[0]["session"]["run_id"], run_id.as_str());
    assert_eq!(launches[0]["node"], Value::Null);
    assert_eq!(launches_of(&control, "session_preamble", &manager).len(), 1);

    let env = env_of(envs.path(), &manager).await;
    assert!(env.contains("FIXTURE_HELLO=monde"), "{env}");
    assert!(env.contains("TEST_STARTED_BY=bob"), "{env}");
    let prompt = std::fs::read_to_string(
        crate::common::flow::worktree(&daemon, &run_id).join(".pdo/prompts/__manager__-iter-0.md"),
    )
    .unwrap();
    assert!(prompt.starts_with("Ligne de fixture\n\n"), "{prompt}");

    let events = extension_events(&control, |events| {
        session_event(events, "session_started", &manager).is_some()
    })
    .await;
    let started = session_event(&events, "session_started", &manager).unwrap();
    assert_eq!(started["actor"], identified("bob"));
    let relayed = read_ws_until(&mut ws, |seen| {
        session_event(seen, "session_started", &manager).is_some()
    })
    .await;
    assert_eq!(
        session_event(&relayed, "session_started", &manager).unwrap()["actor"],
        identified("bob")
    );

    // Stopping it ends it: the end is announced, with who started it.
    let (status, _) = call_as(
        &daemon,
        reqwest::Method::POST,
        &format!("/runs/{run_id}/manager/stop"),
        "bob",
        None,
    )
    .await;
    assert_eq!(status, 200);
    let events = extension_events(&control, |events| {
        session_event(events, "session_ended", &manager).is_some()
    })
    .await;
    assert_eq!(
        session_event(&events, "session_ended", &manager).unwrap()["actor"],
        identified("bob")
    );
}

/// The manager that starts with the Run is started by the Run's creator, and
/// a Node's session by the Run's creator too.
#[tokio::test]
async fn the_manager_born_with_the_run_and_its_nodes_are_started_by_its_creator() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    daemon.enable_manager().await;
    let run_id = run_of_alice(&daemon).await;
    let manager = format!("pdo-mgr-{run_id}");
    let worker = format!("pdo-{run_id}-worker-iter-1");

    let env = env_of(envs.path(), &manager).await;
    assert!(env.contains("TEST_STARTED_BY=alice"), "{env}");
    let launches = launches_of(&control, "session_env", &manager);
    assert_eq!(launches[0]["started_by"], identified("alice"));

    let events = extension_events(&control, |events| {
        session_event(events, "session_started", &manager).is_some()
            && session_event(events, "session_started", &worker).is_some()
    })
    .await;
    let node = session_event(&events, "session_started", &worker).unwrap();
    assert_eq!(node["actor"], identified("alice"));
    assert_eq!(node["session"]["kind"], "node");
    assert_eq!(node["session"]["node_id"], "worker");
    assert_eq!(node["session"]["iter"], 1);
    // The Node's environment is still what `node_env` gives.
    let env = env_of(envs.path(), &worker).await;
    assert!(env.contains("FIXTURE_HELLO=monde"), "{env}");
    assert!(!env.contains("TEST_STARTED_BY"), "{env}");
}

/// FP 2: Alice and Bob each open the library assistant: two sessions, each
/// with its Actor's environment. An Actor refused on the Pipelines opens none.
#[tokio::test]
async fn two_actors_open_two_library_assistants() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    // Each declares what it edits first, as the interface does: an assistant
    // nobody edits with is the sweep's to reap at any pass.
    focus(&daemon, "alice", "solo").await;
    focus(&daemon, "bob", "draft").await;

    let (status, alice) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 200, "{alice}");
    let (_, bob) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist",
        "bob",
        None,
    )
    .await;
    let alice = alice["session"].as_str().unwrap().to_string();
    let bob = bob["session"].as_str().unwrap().to_string();
    assert_ne!(alice, bob, "one assistant per Actor");

    let env = env_of(envs.path(), &alice).await;
    assert!(env.contains("TEST_STARTED_BY=alice"), "{env}");
    assert!(env.contains("FIXTURE_HELLO=monde"), "{env}");
    let env = env_of(envs.path(), &bob).await;
    assert!(env.contains("TEST_STARTED_BY=bob"), "{env}");
    let launches = launches_of(&control, "session_env", &bob);
    assert_eq!(launches[0]["session"]["kind"], "library_assistant");
    assert_eq!(launches[0]["started_by"], identified("bob"));

    // Alice's second open re-attaches hers.
    let (_, again) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist",
        "alice",
        None,
    )
    .await;
    assert_eq!(again["session"], alice.as_str());
    assert_eq!(again["created"], false);

    // Each focus is its own Actor's.
    let (_, focus) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/libassist/focus",
        "bob",
        None,
    )
    .await;
    assert_eq!(focus["pipeline_id"], "draft");
    let (_, focus) = call_as(
        &daemon,
        reqwest::Method::GET,
        "/sessions/libassist/focus",
        "alice",
        None,
    )
    .await;
    assert_eq!(focus["pipeline_id"], "solo");

    // The assistant saves with its Actor's identity: Alice's save lands in the
    // draft of the template Alice has open, Bob's in the one Bob has open.
    let yaml =
        std::fs::read_to_string(daemon.repo_root().join(".pdo/pipelines/solo.yaml")).unwrap();
    // Bob's template has a file, so a draft to save into (ADR-0088).
    std::fs::write(daemon.repo_root().join(".pdo/pipelines/draft.yaml"), &yaml).unwrap();
    let (status, saved) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist/save",
        "alice",
        Some(json!({ "yaml": yaml })),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["id"], "solo");
    let (status, saved) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist/save",
        "bob",
        Some(json!({ "yaml": yaml })),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["id"], "draft");
    let (status, _) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist/save",
        "dave",
        Some(json!({ "yaml": yaml })),
    )
    .await;
    assert_eq!(status, 409, "Dave has no template open");

    control.set(Refusal::parse("pipeline:open_assistant~carol").unwrap());
    let (status, refused) = call_as(
        &daemon,
        reqwest::Method::POST,
        "/sessions/libassist",
        "carol",
        None,
    )
    .await;
    assert_eq!(status, 403, "{refused}");
    assert!(launches_of(
        &control,
        "session_env",
        &pdo_session::domain::names::libassist_session_name_of(
            &pdo::extension_api::Actor::Identified {
                namespace: "test".into(),
                id: "carol".into(),
            }
        )
    )
    .is_empty());

    // Closing Bob's ends his alone; the end reaches the extension.
    let (status, closed) = call_as(
        &daemon,
        reqwest::Method::DELETE,
        "/sessions/libassist",
        "bob",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(closed["reaped"], true);
    let events = extension_events(&control, |events| {
        session_event(events, "session_ended", &bob).is_some()
    })
    .await;
    assert_eq!(
        session_event(&events, "session_ended", &bob).unwrap()["actor"],
        identified("bob")
    );
    assert!(session_event(&events, "session_ended", &alice).is_none());
}

/// FP 3: on the terminal of a Node, the first to attach and whoever takes the
/// hand are announced on `/ws`; a spectator is not.
#[tokio::test]
async fn taking_control_of_a_terminal_is_announced_and_watching_is_not() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let run_id = run_of_alice(&daemon).await;
    let worker = format!("pdo-{run_id}-worker-iter-1");
    env_of(envs.path(), &worker).await;
    let mut ws = daemon.connect_ws_as("alice").await.unwrap();

    let mut a = pty_as(&daemon, &worker, "A", "alice").await;
    let mut b = pty_as(&daemon, &worker, "B", "bob").await;
    role_until(&mut b, "spectator").await;
    b.send(Message::Text(r#"{"type":"take_control"}"#.into()))
        .await
        .unwrap();
    role_until(&mut b, "pilot").await;
    role_until(&mut a, "spectator").await;
    let mut c = pty_as(&daemon, &worker, "C", "carol").await;
    role_until(&mut c, "spectator").await;
    // A spectator taking control from its own pilot seat changes nothing.
    b.send(Message::Text(r#"{"type":"take_control"}"#.into()))
        .await
        .unwrap();

    let relayed = read_ws_for(&mut ws, Duration::from_secs(3)).await;
    let takers: Vec<&Value> = relayed
        .iter()
        .filter(|e| e["type"] == "session_taken_over")
        .map(|e| &e["actor"]["id"])
        .collect();
    assert_eq!(takers, ["alice", "bob"], "{relayed:#?}");
    let taken = relayed
        .iter()
        .find(|e| e["type"] == "session_taken_over")
        .unwrap();
    assert_eq!(taken["session"]["id"], worker.as_str());
    assert_eq!(taken["session"]["run_id"], run_id.as_str());
    assert_eq!(taken["session"]["node_id"], "worker");
}

/// `actor` declares the template its interface edits.
async fn focus(daemon: &TestDaemon, actor: &str, pipeline: &str) {
    let (status, body) = call_as(
        daemon,
        reqwest::Method::PUT,
        "/sessions/libassist/focus",
        actor,
        Some(json!({ "pipeline_id": pipeline })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

/// The PTY of `session`, from the poste `poste`, as `actor`.
async fn pty_as(daemon: &TestDaemon, session: &str, poste: &str, actor: &str) -> Ws {
    let mut request = format!("ws://{}/sessions/{session}/pty?poste={poste}", daemon.addr)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ACTOR_HEADER, actor.parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws
}

/// Read a PTY socket until it is told the role `role`.
async fn role_until(ws: &mut Ws, role: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "never became {role}");
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(300), ws.next()).await
        else {
            continue;
        };
        let frame: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if frame["type"] == "role" && frame["role"] == role {
            return;
        }
    }
}

/// The messages of `/ws` until `done` holds (10 s budget).
async fn read_ws_until(ws: &mut Ws, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while !done(&seen) {
        assert!(Instant::now() < deadline, "never came on /ws: {seen:#?}");
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(300), ws.next()).await
        else {
            continue;
        };
        if let Ok(Ok(value)) = msg.to_text().map(serde_json::from_str::<Value>) {
            seen.push(value);
        }
    }
    seen
}

/// Every message of `/ws` for `window`.
async fn read_ws_for(ws: &mut Ws, window: Duration) -> Vec<Value> {
    let until = Instant::now() + window;
    let mut seen = Vec::new();
    while Instant::now() < until {
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(200), ws.next()).await
        else {
            continue;
        };
        if let Ok(Ok(value)) = msg.to_text().map(serde_json::from_str::<Value>) {
            seen.push(value);
        }
    }
    seen
}
