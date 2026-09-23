//! Layer 3a — shared terminal (#869, story #867, ADR-0075) over the real
//! terminal socket `WS /sessions/<id>/pty?poste=<id>` and a real tmux server.
//!
//! What is observed is external only: the role frames each socket receives, the
//! tmux window's real size, the client flags tmux reports, and what reaches the
//! pane. Between two postes the first arrived pilots: its size is the window's,
//! a spectator's client carries `ignore-size` and its keystrokes are dropped.

use std::time::Duration;

use crate::common::TestDaemon;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmux(socket: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .arg("-L")
        .arg(socket)
        .args(args)
        .output()
        .expect("failed to run tmux");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A session whose pane just sleeps: the tty echoes what reaches it, so
/// `capture-pane` shows typed input. Not `cat`: a pane running `cat` dies with the
/// first client that leaves, which would end the session under the test.
fn create_session(socket: &str, name: &str) {
    let _ = tmux(socket, &["kill-session", "-t", name]);
    let status = std::process::Command::new("tmux")
        .args(["-L", socket, "new-session", "-d", "-s", name, "sleep 600"])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
}

/// `(width, height)` of the session's window.
fn window_size(socket: &str, session: &str) -> (u16, u16) {
    let out = tmux(
        socket,
        &[
            "display",
            "-p",
            "-t",
            session,
            "#{window_width}x#{window_height}",
        ],
    );
    let (w, h) = out.split_once('x').unwrap_or(("0", "0"));
    (w.parse().unwrap_or(0), h.parse().unwrap_or(0))
}

/// The window has the size of a client of `cols × rows`: the status line (when
/// the user's tmux config keeps it) takes one row off the window.
fn follows(size: (u16, u16), cols: u16, rows: u16) -> bool {
    size.0 == cols && (size.1 == rows || size.1 + 1 == rows)
}

async fn wait_window(socket: &str, session: &str, cols: u16, rows: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let size = window_size(socket, session);
        if follows(size, cols, rows) {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("window of {session} is {size:?}, expected it to follow {cols}x{rows}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `client_flags` of every client attached to `session`.
fn client_flags(socket: &str, session: &str) -> Vec<String> {
    tmux(
        socket,
        &["list-clients", "-t", session, "-F", "#{client_flags}"],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

async fn wait_ignoring_clients(socket: &str, session: &str, want: usize, total: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let flags = client_flags(socket, session);
        let ignoring = flags.iter().filter(|f| f.contains("ignore-size")).count();
        if flags.len() == total && ignoring == want {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("expected {want}/{total} clients with ignore-size on {session}, got {flags:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn connect(daemon: &TestDaemon, session: &str, poste: Option<&str>) -> Ws {
    let url = match poste {
        Some(p) => format!("ws://{}/sessions/{session}/pty?poste={p}", daemon.addr),
        None => format!("ws://{}/sessions/{session}/pty", daemon.addr),
    };
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WS connect should succeed");
    ws
}

async fn resize(ws: &mut Ws, cols: u16, rows: u16) {
    ws.send(Message::Text(
        format!(r#"{{"type":"resize","cols":{cols},"rows":{rows}}}"#).into(),
    ))
    .await
    .unwrap();
}

/// Read frames until a role frame matching `want` arrives; return it.
async fn role_until(ws: &mut Ws, want: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                if v["type"] == "role" {
                    if want(&v) {
                        return v;
                    }
                    seen.push(v);
                }
            }
            Ok(Some(Ok(_))) => {}
            _ => panic!("no matching role frame; saw {seen:?}"),
        }
    }
}

fn is(role: &'static str) -> impl Fn(&serde_json::Value) -> bool {
    move |v| v["role"] == role
}

fn spectator_of(cols: u16, rows: u16) -> impl Fn(&serde_json::Value) -> bool {
    move |v| v["role"] == "spectator" && v["pilot"]["cols"] == cols && v["pilot"]["rows"] == rows
}

/// Drain whatever is pending, so a later `role_until` only sees new frames.
async fn assert_no_role_frame(ws: &mut Ws, within: Duration) {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(v["type"], "role", "unexpected role frame {v}");
            }
            Ok(Some(Ok(_))) => {}
            _ => return,
        }
    }
}

#[tokio::test]
async fn the_window_follows_the_pilot_and_ignores_the_spectator() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-shared-term-size";
    create_session(&socket, session);

    // A alone: solo, the window takes its size — today's behaviour.
    let mut a = connect(&daemon, session, Some("posteA")).await;
    role_until(&mut a, is("solo")).await;
    resize(&mut a, 120, 40).await;
    wait_window(&socket, session, 120, 40).await;

    // B arrives, smaller: spectator of A's grid; A pilots, watched once.
    let mut b = connect(&daemon, session, Some("posteB")).await;
    resize(&mut b, 90, 30).await;
    role_until(&mut b, spectator_of(120, 40)).await;
    let pilot = role_until(&mut a, is("pilot")).await;
    assert_eq!(pilot["spectators"], 1);
    wait_ignoring_clients(&socket, session, 1, 2).await;

    // B resizes: the window does not move.
    resize(&mut b, 60, 20).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        follows(window_size(&socket, session), 120, 40),
        "a spectator's resize must not weigh on the window: {:?}",
        window_size(&socket, session)
    );

    // A grows: the window follows, and B is told the new grid.
    resize(&mut a, 150, 45).await;
    wait_window(&socket, session, 150, 45).await;
    role_until(&mut b, spectator_of(150, 45)).await;

    let _ = a.close(None).await;
    let _ = b.close(None).await;
    let _ = tmux(&socket, &["kill-session", "-t", session]);
}

#[tokio::test]
async fn a_spectators_keystrokes_never_reach_the_pane() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-shared-term-input";
    create_session(&socket, session);

    let mut a = connect(&daemon, session, Some("posteA")).await;
    role_until(&mut a, is("solo")).await;
    let mut b = connect(&daemon, session, Some("posteB")).await;
    role_until(&mut b, is("spectator")).await;
    role_until(&mut a, is("pilot")).await;

    b.send(Message::Binary(b"spectator-typed\r".to_vec().into()))
        .await
        .unwrap();
    a.send(Message::Binary(b"pilot-typed\r".to_vec().into()))
        .await
        .unwrap();

    // The pilot's line lands; by then the spectator's (sent first) would have too.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut pane = String::new();
    while tokio::time::Instant::now() < deadline {
        pane = tmux(&socket, &["capture-pane", "-p", "-t", session]);
        if pane.contains("pilot-typed") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        pane.contains("pilot-typed"),
        "pilot input missing: {pane:?}"
    );
    assert!(
        !pane.contains("spectator-typed"),
        "a spectator's input reached the pane: {pane:?}"
    );

    let _ = a.close(None).await;
    let _ = b.close(None).await;
    let _ = tmux(&socket, &["kill-session", "-t", session]);
}

#[tokio::test]
async fn when_the_pilot_leaves_the_hand_goes_to_the_first_arrived_then_solo() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-shared-term-handover";
    create_session(&socket, session);

    let mut a = connect(&daemon, session, Some("posteA")).await;
    role_until(&mut a, is("solo")).await;
    resize(&mut a, 130, 40).await;
    wait_window(&socket, session, 130, 40).await;

    let mut b = connect(&daemon, session, Some("posteB")).await;
    resize(&mut b, 90, 30).await;
    role_until(&mut b, spectator_of(130, 40)).await;
    let mut c = connect(&daemon, session, Some("posteC")).await;
    resize(&mut c, 70, 25).await;
    role_until(&mut c, spectator_of(130, 40)).await;
    wait_ignoring_clients(&socket, session, 2, 3).await;

    // A leaves: B arrived first among the rest, it pilots; the window takes its size.
    let _ = a.close(None).await;
    drop(a);
    let pilot = role_until(&mut b, is("pilot")).await;
    assert_eq!(pilot["spectators"], 1);
    role_until(&mut c, spectator_of(90, 30)).await;
    wait_ignoring_clients(&socket, session, 1, 2).await;
    wait_window(&socket, session, 90, 30).await;

    // B leaves: C is alone, solo again, and the window is its own.
    let _ = b.close(None).await;
    drop(b);
    role_until(&mut c, is("solo")).await;
    wait_ignoring_clients(&socket, session, 0, 1).await;
    wait_window(&socket, session, 70, 25).await;

    let _ = c.close(None).await;
    let _ = tmux(&socket, &["kill-session", "-t", session]);
}

#[tokio::test]
async fn two_sockets_of_one_poste_have_no_role_and_both_weigh_on_the_size() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-shared-term-same-poste";
    create_session(&socket, session);

    let mut t1 = connect(&daemon, session, Some("posteA")).await;
    role_until(&mut t1, is("solo")).await;
    let mut t2 = connect(&daemon, session, Some("posteA")).await;
    role_until(&mut t2, is("solo")).await;
    assert_no_role_frame(&mut t1, Duration::from_millis(300)).await;
    resize(&mut t1, 100, 30).await;
    resize(&mut t2, 100, 30).await;
    wait_window(&socket, session, 100, 30).await;
    wait_ignoring_clients(&socket, session, 0, 2).await;

    // Both tabs type.
    t2.send(Message::Binary(b"second-tab\r".to_vec().into()))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut pane = String::new();
    while tokio::time::Instant::now() < deadline && !pane.contains("second-tab") {
        pane = tmux(&socket, &["capture-pane", "-p", "-t", session]);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(pane.contains("second-tab"), "{pane:?}");

    let _ = t1.close(None).await;
    let _ = t2.close(None).await;
    let _ = tmux(&socket, &["kill-session", "-t", session]);
}

#[tokio::test]
async fn a_socket_without_poste_counts_as_a_poste_of_its_own() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-shared-term-anonymous";
    create_session(&socket, session);

    let mut x = connect(&daemon, session, None).await;
    role_until(&mut x, is("solo")).await;
    let mut y = connect(&daemon, session, None).await;
    role_until(&mut y, is("spectator")).await;
    role_until(&mut x, is("pilot")).await;

    let _ = x.close(None).await;
    let _ = y.close(None).await;
    let _ = tmux(&socket, &["kill-session", "-t", session]);
}

/// ADR-0069 × ADR-0075: only the pilot's Enter lifts a declared wait.
#[tokio::test]
async fn a_spectators_enter_does_not_lift_a_declared_wait() {
    use crate::declared_wait::{create_run, get_json, seed, wait_node_status, wait_user, PARENT};

    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let (status, body) = wait_user(&daemon, &run_id, "worker", Some("Strip or card?")).await;
    assert_eq!(status, 200, "{body}");

    let session = pdo_daemon::tmux_session_manager::node_session_name(&run_id, "worker", 1);
    let mut a = connect(&daemon, &session, Some("posteA")).await;
    role_until(&mut a, is("solo")).await;
    let mut b = connect(&daemon, &session, Some("posteB")).await;
    role_until(&mut b, is("spectator")).await;

    b.send(Message::Binary(b"card\r".to_vec().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(
        run["nodes"]["worker"]["status"], "awaiting_user",
        "a spectator's Enter must not lift the wait: {run}"
    );

    a.send(Message::Binary(b"strip\r".to_vec().into()))
        .await
        .unwrap();
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let _ = a.close(None).await;
    let _ = b.close(None).await;
}
