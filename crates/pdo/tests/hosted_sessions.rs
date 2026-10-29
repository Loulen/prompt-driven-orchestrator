//! API 0.19 — **the Sessions hébergées** (ADR-0092), on a real daemon with the
//! test extension and a real tmux: opened and terminated through the
//! extension's Host, launched with nothing of a Run and nothing written into
//! their folder, their changes on `/ws` and in `on_event`, refused by the
//! Autorisation d'opération and by a full Cap, taken back by a restarted
//! daemon, and their terminal opened like any other; relaunched, configured,
//! typed into and forgotten, their Artefacts live listed and read, their wait
//! declared by `pdo wait-user` and lifted by an Enter, `pdo complete` refused
//! in them, terminated when their extension is gone, and their Zoom du
//! terminal set by the API and the Host.

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

async fn post_as(url: &str, path: &str, actor: &str, body: Option<Value>) -> (u16, Value) {
    call_as(url, reqwest::Method::POST, path, actor, body).await
}

async fn get_as(url: &str, path: &str, actor: &str) -> (u16, Value) {
    call_as(url, reqwest::Method::GET, path, actor, None).await
}

/// The session `key`, once `check` holds of it.
async fn session_when(url: &str, key: &str, check: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (_, session) = get_as(url, &format!("/ext/test/sessions/{key}"), "alice").await;
        if check(&session) {
            return session;
        }
        assert!(Instant::now() < deadline, "never came: {session:#}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The value of `name` in the environment `env` holds.
fn var(env: &str, name: &str) -> String {
    env.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
        .unwrap_or_default()
        .to_string()
}

/// FP 1 and 3: a relaunched agent is the same session, started by who
/// relaunched it, with its `PDO_LIVE_DIR` kept; its Artefacts live are listed,
/// read, published, and forgotten with it.
#[tokio::test]
async fn a_session_relaunches_keeps_its_artefacts_live_and_is_forgotten() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let url = daemon.url();
    let folder = tempfile::tempdir().unwrap();
    let (status, session) = open_as(&url, "alice", in_path("r", "agent", folder.path())).await;
    assert_eq!(status, 201, "{session}");
    let id = session["id"].as_str().unwrap().to_string();
    let env = env_of(envs.path(), &id).await;
    let live = PathBuf::from(var(&env, "PDO_LIVE_DIR"));
    assert_eq!(
        live,
        daemon
            .repo_root()
            .join("home/.pdo/hosted")
            .join(&id)
            .join("live")
    );
    assert!(live.is_dir());

    std::fs::write(live.join("plan.md"), "# Plan").unwrap();
    let (_, listed) = get_as(&url, "/ext/test/sessions/r/live", "alice").await;
    assert_eq!(listed[0]["path"], "plan.md", "{listed}");
    let read = reqwest::get(format!("{url}/ext/test/sessions/r/live/plan.md"))
        .await
        .unwrap();
    assert_eq!(read.status().as_u16(), 200);
    assert_eq!(read.text().await.unwrap(), "# Plan");
    let events = extension_events(&control, |events| {
        events
            .iter()
            .any(|e| e["type"] == "hosted_live_artifacts_changed" && e["id"] == id.as_str())
    })
    .await;
    let change = events
        .iter()
        .find(|e| e["type"] == "hosted_live_artifacts_changed")
        .unwrap();
    assert_eq!(change["key"], "r");
    assert_eq!(change["artifacts"][0]["path"], "plan.md");

    let (status, refused) = post_as(&url, "/ext/test/sessions/r/relaunch", "bob", None).await;
    assert_eq!(
        status, 409,
        "a running session does not relaunch: {refused}"
    );
    call_as(
        &url,
        reqwest::Method::DELETE,
        "/ext/test/sessions/r",
        "alice",
        None,
    )
    .await;
    std::fs::remove_file(envs.path().join(format!("{id}.env"))).unwrap();
    let (status, relaunched) = post_as(&url, "/ext/test/sessions/r/relaunch", "bob", None).await;
    assert_eq!(status, 200, "{relaunched}");
    assert_eq!(relaunched["id"], id.as_str(), "the same session");
    assert_eq!(relaunched["state"], "running");
    assert_eq!(relaunched["started_by"]["id"], "bob");
    let env = env_of(envs.path(), &id).await;
    assert_eq!(var(&env, "TEST_STARTED_BY"), "bob", "{env}");
    assert_eq!(var(&env, "PDO_LIVE_DIR"), live.to_string_lossy());
    assert!(live.join("plan.md").is_file(), "kept across the relaunch");
    assert!(control.asked().iter().any(|(actor, op)| op.verb
        == pdo::extension_api::Verb::Action("relaunch".into())
        && matches!(actor, pdo::extension_api::Actor::Identified { id, .. } if id == "bob")));
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);

    let (status, _) = post_as(&url, "/ext/test/sessions/r/forget", "alice", None).await;
    assert_eq!(status, 409, "a live session is not forgotten");
    call_as(
        &url,
        reqwest::Method::DELETE,
        "/ext/test/sessions/r",
        "alice",
        None,
    )
    .await;
    let (status, forgotten) = post_as(&url, "/ext/test/sessions/r/forget", "alice", None).await;
    assert_eq!(status, 200, "{forgotten}");
    assert!(!live.exists(), "its state is erased");
    let (status, _) = get_as(&url, "/ext/test/sessions/r", "alice").await;
    assert_eq!(status, 404);
    extension_events(&control, |events| {
        events
            .iter()
            .any(|e| e["type"] == "hosted_session_forgotten" && e["session"]["id"] == id.as_str())
    })
    .await;
    let (status, _) = open_as(&url, "alice", in_path("r", "agent", folder.path())).await;
    assert_eq!(status, 201, "its key is free again");
}

/// A configuration is recorded for the next relaunch; a shell has none.
#[tokio::test]
async fn a_configuration_is_recorded_and_a_shell_has_none() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let url = daemon.url();
    let home = tempfile::tempdir().unwrap();
    open_as(&url, "alice", in_path("a", "agent", home.path())).await;
    open_as(&url, "alice", in_path("s", "shell", home.path())).await;
    let config = json!({ "model": "opus", "effort": "high", "prompt": "Deux" });
    let (status, configured) = call_as(
        &url,
        reqwest::Method::PUT,
        "/ext/test/sessions/a/config",
        "alice",
        Some(config.clone()),
    )
    .await;
    assert_eq!(status, 200, "{configured}");
    assert_eq!(configured["config"], config);
    assert_eq!(configured["state"], "running");
    assert!(control
        .asked()
        .iter()
        .any(|(_, op)| op.verb == pdo::extension_api::Verb::Action("configure".into())));
    let (status, refused) = call_as(
        &url,
        reqwest::Method::PUT,
        "/ext/test/sessions/s/config",
        "alice",
        Some(config),
    )
    .await;
    assert_eq!(status, 400, "{refused}");
    let (status, refused) = call_as(
        &url,
        reqwest::Method::PUT,
        "/ext/test/sessions/a/config",
        "alice",
        Some(json!({ "harness": "nope" })),
    )
    .await;
    assert_eq!(status, 400, "{refused}");
}

/// FP 2: a text laid in a session waits in its input, unsubmitted; refused,
/// nothing is laid.
#[tokio::test]
async fn a_text_waits_in_the_input_until_someone_presses_enter() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let url = daemon.url();
    let home = tempfile::tempdir().unwrap();
    let (_, shell) = open_as(&url, "alice", in_path("t", "shell", home.path())).await;
    let id = shell["id"].as_str().unwrap().to_string();
    let socket = daemon.tmux_socket();
    let out = home.path().join("typed");
    let command = format!("echo typed > '{}'", out.display());
    // Once the shell prompts, the text lands in its input line.
    let deadline = Instant::now() + Duration::from_secs(15);
    while !String::from_utf8_lossy(&tmux(&socket, &["capture-pane", "-p", "-t", &id]).stdout)
        .contains('$')
    {
        assert!(Instant::now() < deadline, "the shell never prompted");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (status, body) = post_as(
        &url,
        "/ext/test/sessions/t/type",
        "alice",
        Some(json!({ "text": command })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&tmux(&socket, &["capture-pane", "-p", "-t", &id]).stdout)
        .contains("echo typed")
    {
        assert!(Instant::now() < deadline, "never laid");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!out.exists(), "not submitted");
    tmux(&socket, &["send-keys", "-t", &id, "Enter"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !out.exists() {
        assert!(Instant::now() < deadline, "never submitted");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    control.set(Refusal::parse("session:type").unwrap());
    let (status, _) = post_as(
        &url,
        "/ext/test/sessions/t/type",
        "alice",
        Some(json!({ "text": "nope" })),
    )
    .await;
    assert_eq!(status, 403);
}

/// Run the real `pdo` binary with `args`, inside the Session hébergée `id`
/// of `daemon` started by alice.
async fn pdo_in_session(daemon: &TestDaemon, id: &str, args: &[&str]) -> (Option<i32>, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (url, id) = (daemon.url(), id.to_string());
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_pdo"))
            .args(&args)
            .env_remove("PDO_RUN_ID")
            .env_remove("PDO_NODE_ID")
            .env_remove("PDO_NODE_ITER")
            .env("PDO_SESSION_ID", &id)
            .env("PDO_DAEMON_URL", &url)
            .env("PDO_TOKEN", "test-session.alice")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("panicked"), "{stderr}");
    (output.status.code(), stderr)
}

/// FP 4 and 5: `pdo wait-user` in a Session hébergée declares its wait, an
/// Enter typed in its terminal lifts it; `pdo complete` is refused and the
/// session goes on.
#[tokio::test]
async fn wait_user_declares_the_wait_and_complete_is_refused_in_a_hosted_session() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let url = daemon.url();
    let home = tempfile::tempdir().unwrap();
    let (_, shell) = open_as(&url, "alice", in_path("w", "shell", home.path())).await;
    let id = shell["id"].as_str().unwrap().to_string();

    let (code, stderr) =
        pdo_in_session(&daemon, &id, &["wait-user", "--message", "Quelle base ?"]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.contains("now awaiting the user"), "{stderr}");
    let waiting = session_when(&url, "w", |s| s["state"] == "awaiting_user").await;
    assert_eq!(waiting["wait_message"], "Quelle base ?");
    let wait = control
        .asked()
        .into_iter()
        .find(|(_, op)| op.verb == pdo::extension_api::Verb::Action("wait_user".into()))
        .unwrap();
    assert!(
        matches!(&wait.0, pdo::extension_api::Actor::Identified { id, .. } if id == "alice"),
        "the agent acts as who started it: {:?}",
        wait.0
    );
    let (code, stderr) =
        pdo_in_session(&daemon, &id, &["wait-user", "--message", "Quelle base ?"]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.contains("legal no-op"), "{stderr}");

    let (code, stderr) = pdo_in_session(&daemon, &id, &["complete"]).await;
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stderr.contains("REFUSED"), "{stderr}");
    assert!(stderr.contains("hosted session"), "{stderr}");
    for command in ["fail", "skip"] {
        let (code, stderr) = pdo_in_session(&daemon, &id, &[command, "--reason", "x"]).await;
        assert_eq!(code, Some(3), "{command}: {stderr}");
    }
    let (_, still) = get_as(&url, "/ext/test/sessions/w", "alice").await;
    assert_eq!(still["state"], "awaiting_user", "the session goes on");
    assert!(tmux_has(&daemon.tmux_socket(), &id));

    let pty = format!("ws://{}/sessions/{id}/pty", daemon.addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&pty).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"resize","cols":100,"rows":30}"#.into(),
    ))
    .await
    .unwrap();
    ws.send(Message::Binary(b"postgres".to_vec().into()))
        .await
        .unwrap();
    ws.send(Message::Binary(b"\r".to_vec().into()))
        .await
        .unwrap();
    let resumed = session_when(&url, "w", |s| s["state"] == "running").await;
    assert!(resumed.get("wait_message").is_none(), "{resumed}");

    call_as(
        &url,
        reqwest::Method::DELETE,
        "/ext/test/sessions/w",
        "alice",
        None,
    )
    .await;
    let (code, stderr) = pdo_in_session(&daemon, &id, &["wait-user"]).await;
    assert_eq!(code, Some(3), "an ended session waits for nobody: {stderr}");
    assert!(stderr.contains("hosted_session_not_running"), "{stderr}");

    // The route the CLI calls: a name no session holds is named.
    let unknown = reqwest::Client::new()
        .post(format!("{url}/sessions/pdo-hosted-nobody/wait-user"))
        .json(&json!({ "message": "anyone?" }))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status().as_u16(), 404);
    let body: Value = unknown.json().await.unwrap();
    assert_eq!(body["error"], "hosted_session_not_found");
}

/// A daemon without the extension of a live Session hébergée terminates it
/// at boot and keeps its record.
#[tokio::test]
async fn a_daemon_without_its_extension_terminates_the_session_and_keeps_it() {
    let root = tempfile::tempdir().unwrap();
    let envs = tempfile::tempdir().unwrap();
    let (first, _) = serve_on(root.path(), 0, envs.path()).await;
    let port = first.addr.port();
    let url = format!("http://{}", first.addr);
    let socket = pdo_session::domain::names::tmux_socket_name(port);
    let home = tempfile::tempdir().unwrap();
    let (status, shell) = open_as(&url, "alice", in_path("orphan", "shell", home.path())).await;
    assert_eq!(status, 201, "{shell}");
    let id = shell["id"].as_str().unwrap().to_string();
    first.task.abort();
    let _ = first.task.await;

    let mut config = DaemonConfig {
        tmux_cmd_override: Some(env_tail(envs.path())),
        sandbox_home_override: Some(root.path().join("home")),
        ..DaemonConfig::default()
    };
    pdo::compose(&mut config, Vec::new(), None).unwrap();
    let second = serve_with_config(
        SocketAddr::from(([127, 0, 0, 1], port)),
        root.path().into(),
        config,
    )
    .await
    .unwrap();
    assert!(!tmux_has(&socket, &id), "its process is killed");
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.path()
                .join("home/.pdo/hosted")
                .join(&id)
                .join("session.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record["state"], "ended");
    assert_eq!(record["end_reason"], "terminated");
    assert_eq!(record["key"], "orphan");
    second.task.abort();
    tmux(&socket, &["kill-server"]);
}

/// The messages of `ws` until `done` holds of them, or `secs` went by.
async fn read_messages(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    secs: u64,
    done: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        if let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(200), ws.next()).await
        {
            if let Ok(Ok(value)) = msg.to_text().map(serde_json::from_str::<Value>) {
                seen.push(value);
            }
        }
    }
    seen
}

/// The zooms the changes of `id` carried, in order.
fn zooms_of(messages: &[Value], id: &str) -> Vec<u64> {
    messages
        .iter()
        .filter(|m| m["type"] == "hosted_session_changed" && m["session"]["id"] == id)
        .filter_map(|m| m["session"]["terminal_zoom"].as_u64())
        .collect()
}

/// CORE-124: the Zoom du terminal of a Session hébergée is set by the API
/// (any Poste) and by the Host, an Operation `session` × `zoom` in its
/// Projet, out of range refused; its change, and every Operation on the
/// session, reach on `/ws` only the connections that may read it.
#[tokio::test]
async fn the_zoom_of_a_terminal_is_set_by_the_api_and_the_host_and_reaches_who_may_read_it() {
    let envs = tempfile::tempdir().unwrap();
    let (daemon, control) = daemon(envs.path()).await;
    let url = daemon.url();
    let home = tempfile::tempdir().unwrap();
    control.set(Refusal::parse("session:read~bob").unwrap());
    let mut alice_ws = daemon.connect_ws_as("alice").await.unwrap();
    let mut bob_ws = daemon.connect_ws_as("bob").await.unwrap();
    let mut body = in_path("z", "shell", home.path());
    body["project"] = json!("p1");
    let (status, shell) = open_as(&url, "alice", body).await;
    assert_eq!(status, 201, "{shell}");
    assert_eq!(shell["terminal_zoom"], 100);
    let id = shell["id"].as_str().unwrap().to_string();
    let zoom_path = format!("/sessions/{id}/terminal-zoom");

    let (status, read) = get_as(&url, &zoom_path, "alice").await;
    assert_eq!(
        (status, read),
        (200, json!({ "zoom": 100, "hosted": true }))
    );
    let (status, _) = get_as(&url, &zoom_path, "bob").await;
    assert_eq!(status, 403, "bob may not read the session");
    let (status, other) = get_as(&url, "/sessions/pdo-r1-impl-iter-1/terminal-zoom", "bob").await;
    assert_eq!(
        (status, other),
        (200, json!({ "zoom": 100, "hosted": false })),
        "any other session is at 100 %"
    );

    let put = |actor: &'static str, path: String, zoom: Value| {
        let url = url.clone();
        async move {
            call_as(
                &url,
                reqwest::Method::PUT,
                &path,
                actor,
                Some(json!({ "zoom": zoom })),
            )
            .await
        }
    };
    let (status, set) = put("alice", zoom_path.clone(), json!(150)).await;
    assert_eq!((status, set), (200, json!({ "zoom": 150, "hosted": true })));
    let zoom = control
        .asked()
        .into_iter()
        .rev()
        .find(|(_, op)| op.verb == pdo::extension_api::Verb::Action("zoom".into()))
        .unwrap();
    assert_eq!(zoom.1.target.as_deref(), Some(id.as_str()));
    assert_eq!(zoom.1.projects, ["p1"]);
    let (status, _) = put("alice", zoom_path.clone(), json!(20)).await;
    assert_eq!(status, 400, "out of range");
    let (status, _) = put(
        "alice",
        "/sessions/pdo-hosted-none/terminal-zoom".into(),
        json!(100),
    )
    .await;
    assert_eq!(status, 404, "not a hosted session");

    // By the extension's Host.
    let (status, zoomed) = put("alice", "/ext/test/sessions/z/zoom".into(), json!(75)).await;
    assert_eq!(status, 200, "{zoomed}");
    assert_eq!(zoomed["terminal_zoom"], 75);
    let (_, read) = get_as(&url, &zoom_path, "alice").await;
    assert_eq!(read["zoom"], 75);

    // Refused: nothing changes.
    control.set(Refusal::parse("session:zoom,session:read~bob").unwrap());
    let (status, refused) = put("alice", zoom_path.clone(), json!(200)).await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(refused["error"], "operation_refused");
    let (_, read) = get_as(&url, &zoom_path, "alice").await;
    assert_eq!(read["zoom"], 75);

    let seen = read_messages(&mut alice_ws, 10, |seen| zooms_of(seen, &id).contains(&75)).await;
    assert_eq!(
        zooms_of(&seen, &id).last(),
        Some(&75),
        "every Poste follows"
    );
    assert!(
        seen.iter().any(|m| m["type"] == "operation_applied"
            && m["operation"]["resource"] == "session"
            && m["operation"]["verb"]["action"] == "zoom"),
        "{seen:#?}"
    );
    let bob_saw = read_messages(&mut bob_ws, 2, |_| false).await;
    let about_the_session: Vec<_> = bob_saw
        .iter()
        .filter(|m| {
            m["session"]["id"] == id.as_str()
                || (m["type"] == "operation_applied" && m["operation"]["resource"] == "session")
        })
        .collect();
    assert!(
        about_the_session.is_empty(),
        "who may not read the session hears nothing of it, its opening included: {about_the_session:#?}"
    );
}
