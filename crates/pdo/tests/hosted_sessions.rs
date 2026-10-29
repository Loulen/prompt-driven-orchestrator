//! API 0.17 — **the Sessions hébergées** (ADR-0091), on a real daemon with the
//! test extension and a real tmux: opened and terminated through the
//! extension's Host, launched with nothing of a Run and nothing written into
//! their folder, their changes on `/ws` and in `on_event`, refused by the
//! Autorisation d'opération and by a full Cap, taken back by a restarted
//! daemon, and their terminal opened like any other.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use pdo::{serve_with_config, DaemonConfig, DaemonHandle};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::common::{lock_session_cap, TestDaemon};
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// A daemon with the test extension whose agent sessions write their
/// environment to `<envs>/<session name>.env`, then sleep.
async fn daemon(envs: &Path) -> (TestDaemon, Control) {
    let tail = env_tail(envs);
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(
        |_| Ok(()),
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

fn env_tail(envs: &Path) -> String {
    format!(
        "env > '{}'/\"$(tmux display-message -p '#S')\".env; exec sleep 600",
        envs.display()
    )
}

/// `method path` as `actor` (the test extension's header).
async fn call_as(
    url: &str,
    method: reqwest::Method,
    path: &str,
    actor: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{url}{path}"))
        .header(ACTOR_HEADER, actor);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn open_as(url: &str, actor: &str, body: Value) -> (u16, Value) {
    call_as(
        url,
        reqwest::Method::POST,
        "/ext/test/sessions",
        actor,
        Some(body),
    )
    .await
}

fn in_path(key: &str, kind: &str, dir: &Path) -> Value {
    json!({ "key": key, "kind": kind, "folder": { "kind": "path", "path": dir } })
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

fn tmux(socket: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new("tmux")
        .args(["-L", socket])
        .args(args)
        .output()
        .unwrap()
}

fn tmux_has(socket: &str, name: &str) -> bool {
    tmux(socket, &["has-session", "-t", name]).status.success()
}

fn changes_of(events: &[Value], id: &str) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["type"] == "hosted_session_changed" && e["session"]["id"] == id)
        .map(|e| {
            e["session"]["state"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

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

/// The changes of `id` on `ws`, until `count` of them came (or two seconds
/// went by without them, for `0`).
async fn read_changes(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: &str,
    count: usize,
) -> Vec<String> {
    let window = if count == 0 { 2 } else { 10 };
    let deadline = Instant::now() + Duration::from_secs(window);
    let mut seen = Vec::new();
    while Instant::now() < deadline && (count == 0 || changes_of(&seen, id).len() < count) {
        if let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(200), ws.next()).await
        {
            if let Ok(Ok(value)) = msg.to_text().map(serde_json::from_str::<Value>) {
                seen.push(value);
            }
        }
    }
    changes_of(&seen, id)
}

/// FP 1 and 4: an agent opens in an empty folder and in a repository, reads
/// only the extensions' text and its prompt, carries only its session's
/// variables, writes nothing in either folder; terminating it ends it.
#[tokio::test]
async fn an_agent_opens_touching_neither_its_folder_nor_a_run_and_terminates() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    control.launch.set_preamble(Some("Ligne de fixture".into()));
    let mut ws = daemon.connect_ws_as("alice").await.unwrap();
    let empty = tempfile::tempdir().unwrap();

    let mut body = in_path("a1", "agent", empty.path());
    body["config"] = json!({ "prompt": "Bonjour" });
    let (status, session) = open_as(&daemon.url(), "alice", body).await;
    assert_eq!(status, 201, "{session}");
    assert_eq!(session["state"], "running");
    assert_eq!(session["started_by"]["id"], "alice");
    let id = session["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("pdo-hosted-"), "{id}");

    let env = env_of(envs.path(), &id).await;
    assert!(env.contains(&format!("PDO_SESSION_ID={id}")), "{env}");
    assert!(env.contains("PDO_DAEMON_URL=http://localhost:"), "{env}");
    assert!(env.contains("FIXTURE_HELLO=monde"), "{env}");
    assert!(env.contains("TEST_STARTED_BY=alice"), "{env}");
    assert!(!env.contains("PDO_RUN_ID="), "{env}");
    assert!(!env.contains("PDO_NODE_ID="), "{env}");
    assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
    let primer = daemon
        .repo_root()
        .join("home/.pdo/hosted")
        .join(&id)
        .join("primer.md");
    assert_eq!(
        std::fs::read_to_string(primer).unwrap(),
        "Ligne de fixture\n\nBonjour"
    );
    let launch = control
        .launch
        .launches()
        .into_iter()
        .find(|l| l["hook"] == "session_env" && l["launch"]["session"]["id"] == id.as_str())
        .unwrap();
    assert_eq!(launch["launch"]["session"]["kind"], "hosted");
    assert_eq!(launch["launch"]["session"]["key"], "a1");
    assert_eq!(launch["launch"]["started_by"]["id"], "alice");

    // In a repository: the tree stays clean.
    let repo = tempfile::tempdir().unwrap();
    crate::common::flow::git_init(repo.path()).unwrap();
    let (status, in_repo) =
        open_as(&daemon.url(), "alice", in_path("a2", "agent", repo.path())).await;
    assert_eq!(status, 201, "{in_repo}");
    env_of(envs.path(), in_repo["id"].as_str().unwrap()).await;
    assert_eq!(
        crate::common::flow::git(repo.path(), &["status", "--porcelain"]),
        ""
    );

    let events = extension_events(&control, |e| changes_of(e, &id).len() >= 2).await;
    assert_eq!(changes_of(&events, &id), ["starting", "running"]);
    assert_eq!(read_changes(&mut ws, &id, 2).await, ["starting", "running"]);

    let (status, ended) = call_as(
        &daemon.url(),
        reqwest::Method::DELETE,
        "/ext/test/sessions/a1",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, 200, "{ended}");
    assert_eq!(ended["state"], "ended");
    assert_eq!(ended["end_reason"], "terminated");
    assert!(!tmux_has(&daemon.tmux_socket(), &id));
    let events = extension_events(&control, |e| changes_of(e, &id).len() >= 3).await;
    assert_eq!(changes_of(&events, &id), ["starting", "running", "ended"]);
    let (_, read) = call_as(
        &daemon.url(),
        reqwest::Method::GET,
        "/ext/test/sessions/a1",
        "alice",
        None,
    )
    .await;
    assert_eq!(read["end_reason"], "terminated", "the record stays");
    let operations = control.asked();
    assert!(operations.iter().any(|(_, op)| op.verb
        == pdo::extension_api::Verb::Action("terminate".into())
        && op.target.as_deref() == Some(id.as_str())));
}

/// FP 2: a shell gets the Contribution au lancement of who opened it: the
/// token it carries identifies them.
#[tokio::test]
async fn a_shell_carries_the_token_of_who_opened_it() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let home = tempfile::tempdir().unwrap();
    let (status, shell) =
        open_as(&daemon.url(), "alice", in_path("sh", "shell", home.path())).await;
    assert_eq!(status, 201, "{shell}");
    let id = shell["id"].as_str().unwrap();
    let out = envs.path().join("token");
    let command = format!("printenv PDO_TOKEN > '{}'", out.display());
    let deadline = Instant::now() + Duration::from_secs(15);
    let token = loop {
        tmux(
            &daemon.tmux_socket(),
            &["send-keys", "-t", id, &command, "Enter"],
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Ok(text) = std::fs::read_to_string(&out) {
            if !text.trim().is_empty() {
                break text.trim().to_string();
            }
        }
        assert!(Instant::now() < deadline, "the shell never answered");
    };
    assert_eq!(token, "test-session.alice");
    // That token is alice, for the core and its extensions.
    let resp = reqwest::Client::new()
        .post(format!("{}/ext/test/sessions", daemon.url()))
        .bearer_auth(&token)
        .json(&in_path("by-token", "shell", home.path()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);
    let opened: Value = resp.json().await.unwrap();
    assert_eq!(opened["started_by"]["id"], "alice");
}

/// A refused Operation opens nothing; the attach of its terminal is asked in
/// the Projet of its record.
#[tokio::test]
async fn the_gestures_are_operations_in_the_projet_of_the_session() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let home = tempfile::tempdir().unwrap();
    control.set(Refusal::parse("session:open").unwrap());
    let (status, body) = open_as(&daemon.url(), "alice", in_path("no", "shell", home.path())).await;
    assert_eq!(status, 403, "{body}");
    let (_, listed) = call_as(
        &daemon.url(),
        reqwest::Method::GET,
        "/ext/test/sessions",
        "alice",
        None,
    )
    .await;
    assert_eq!(listed, json!([]));

    control.set(Refusal::parse("session:attach@p1,session:read~bob").unwrap());
    let mut alice_ws = daemon.connect_ws_as("alice").await.unwrap();
    let mut bob_ws = daemon.connect_ws_as("bob").await.unwrap();
    let mut body = in_path("yes", "shell", home.path());
    body["project"] = json!("p1");
    let (status, shell) = open_as(&daemon.url(), "alice", body).await;
    assert_eq!(status, 201, "{shell}");
    // Its changes reach the connections that may read it, and no other.
    let id = shell["id"].as_str().unwrap();
    assert_eq!(
        read_changes(&mut alice_ws, id, 2).await,
        ["starting", "running"]
    );
    assert!(read_changes(&mut bob_ws, id, 0).await.is_empty());
    let read = control
        .asked()
        .into_iter()
        .find(|(_, op)| {
            op.verb == pdo::extension_api::Verb::Read && op.target.as_deref() == Some(id)
        })
        .unwrap();
    assert_eq!(read.1.projects, ["p1"]);
    let url = format!(
        "ws://{}/sessions/{}/pty",
        daemon.addr,
        shell["id"].as_str().unwrap()
    );
    let refused = tokio_tungstenite::connect_async(&url).await;
    assert!(refused.is_err(), "the attach is refused in p1");
    let asked = control.asked();
    let attach = asked
        .iter()
        .rev()
        .find(|(_, op)| op.verb == pdo::extension_api::Verb::Action("attach".into()))
        .unwrap();
    assert_eq!(attach.1.projects, ["p1"]);
}

/// FP 5: the Cap counts the Sessions hébergées, and refuses at once.
#[tokio::test]
async fn a_full_cap_refuses_a_hosted_session_at_once() {
    let _cap = lock_session_cap("1");
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let home = tempfile::tempdir().unwrap();
    let (status, _) = open_as(&daemon.url(), "alice", in_path("one", "agent", home.path())).await;
    assert_eq!(status, 201);
    let gauges = daemon.get_json("/sessions").await;
    assert_eq!(gauges["live"], 1, "{gauges}");
    let started = Instant::now();
    let (status, body) =
        open_as(&daemon.url(), "alice", in_path("two", "agent", home.path())).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("session cap reached (1/1"),
        "{body}"
    );
    assert!(started.elapsed() < Duration::from_secs(5), "nothing waits");

    call_as(
        &daemon.url(),
        reqwest::Method::DELETE,
        "/ext/test/sessions/one",
        "alice",
        None,
    )
    .await;
    let (status, body) =
        open_as(&daemon.url(), "alice", in_path("two", "agent", home.path())).await;
    assert_eq!(
        status, 201,
        "a terminated session gives its slot back: {body}"
    );
}

/// A daemon of the test extension on `port` over `root`, whose `HOME` is
/// `root/home`: what a restart on the same port finds.
async fn serve_on(root: &Path, port: u16, envs: &Path) -> (DaemonHandle, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let mut config = DaemonConfig {
        tmux_cmd_override: Some(env_tail(envs)),
        sandbox_home_override: Some(root.join("home")),
        ..DaemonConfig::default()
    };
    pdo::compose(&mut config, ext.boxed(), None).unwrap();
    let handle = serve_with_config(
        SocketAddr::from(([127, 0, 0, 1], port)),
        root.into(),
        config,
    )
    .await
    .unwrap();
    (handle, control)
}

/// FP 3: restarted on the same port, the daemon keeps the session with who
/// started it, and its sweep spares it while killing a stray of the prefix.
#[tokio::test]
async fn a_restarted_daemon_keeps_the_session_and_its_starter() {
    let root = tempfile::tempdir().unwrap();
    let envs = tempfile::tempdir().unwrap();
    let (first, _) = serve_on(root.path(), 0, envs.path()).await;
    let port = first.addr.port();
    let url = format!("http://{}", first.addr);
    let socket = pdo_session::domain::names::tmux_socket_name(port);
    let home = tempfile::tempdir().unwrap();
    let (status, shell) = open_as(&url, "alice", in_path("kept", "shell", home.path())).await;
    assert_eq!(status, 201, "{shell}");
    let id = shell["id"].as_str().unwrap().to_string();
    tmux(
        &socket,
        &["new-session", "-d", "-s", "pdo-hosted-stray", "sleep 600"],
    );
    first.task.abort();
    let _ = first.task.await;

    let (second, control) = serve_on(root.path(), port, envs.path()).await;
    let url = format!("http://{}", second.addr);
    let (_, kept) = call_as(
        &url,
        reqwest::Method::GET,
        "/ext/test/sessions/kept",
        "bob",
        None,
    )
    .await;
    assert_eq!(kept["state"], "running", "{kept}");
    assert_eq!(kept["started_by"]["id"], "alice");
    assert!(tmux_has(&socket, &id), "the session still runs");
    assert!(!tmux_has(&socket, "pdo-hosted-stray"), "a stray is swept");
    second.run_orphan_sweep_tick().await;
    assert!(tmux_has(&socket, &id), "and never swept");

    // Its end is announced with who started it.
    let (status, _) = call_as(
        &url,
        reqwest::Method::DELETE,
        "/ext/test/sessions/kept",
        "bob",
        None,
    )
    .await;
    assert_eq!(status, 200);
    let events = extension_events(&control, |e| {
        e.iter()
            .any(|e| e["type"] == "session_ended" && e["session"]["id"] == id.as_str())
    })
    .await;
    let ended = events
        .iter()
        .find(|e| e["type"] == "session_ended" && e["session"]["id"] == id.as_str())
        .unwrap();
    assert_eq!(ended["actor"]["id"], "alice");
    second.task.abort();
    tmux(&socket, &["kill-server"]);
}

/// The shared terminal opens on a Session hébergée like on any session.
#[tokio::test]
async fn the_terminal_of_a_hosted_shell_answers() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, _control) = daemon(envs.path()).await;
    let home = tempfile::tempdir().unwrap();
    let (_, shell) = open_as(&daemon.url(), "alice", in_path("t", "shell", home.path())).await;
    let id = shell["id"].as_str().unwrap();
    let url = format!("ws://{}/sessions/{id}/pty", daemon.addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"resize","cols":100,"rows":30}"#.into(),
    ))
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut out = String::new();
    while !out.contains("hosted-ok") {
        assert!(Instant::now() < deadline, "never answered: {out:?}");
        ws.send(Message::Binary(b"echo hosted-\"\"ok\r".to_vec().into()))
            .await
            .unwrap();
        let wait = Instant::now() + Duration::from_millis(500);
        while Instant::now() < wait {
            if let Ok(Some(Ok(Message::Binary(bytes)))) =
                tokio::time::timeout(Duration::from_millis(100), ws.next()).await
            {
                out.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
}
