//! Layer 3a — PTY bridge WebSocket integration test.
//!
//! Substitutes Claude with `bash -c 'cat'` inside a tmux session, opens
//! `WS /sessions/<id>/pty`, sends bytes, and asserts roundtrip echo.

mod common;

use std::time::Duration;

use common::TestDaemon;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// The daemon talks to a tmux server scoped to its own socket (`tmux -L`), so
// out-of-band session management must go through the same socket — a session
// created on the default server would be invisible to the PTY bridge.
fn create_tmux_session_with_cat(socket: &str, name: &str) {
    let status = std::process::Command::new("tmux")
        .args(["-L", socket, "new-session", "-d", "-s", name, "cat"])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
}

fn kill_tmux_session(socket: &str, name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["-L", socket, "kill-session", "-t", name])
        .status();
}

/// Layer 3a: open WS /sessions/<id>/pty, send bytes to `cat`, read them back.
#[tokio::test]
async fn pty_ws_roundtrip_echo() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // This test exercises the PTY bridge, not the reaper. The session below is
    // created out-of-band (no run in the event log), so an armed orphan sweep
    // kills it as an unrecognised `pdo-*` name (no TTL on that arm) and the
    // attached client prints `[exited]` instead of the echo — opt out of all
    // automatic cleanup for THIS daemon, per-daemon, never via process-global env
    // (a sibling's `remove_var` used to land inside this `spawn`, cf.
    // `TestDaemon::spawn_nested`).
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    let session_name = "pdo-pty-test-echo";
    // Clean up any leftover from a previous run
    kill_tmux_session(&socket, session_name);
    create_tmux_session_with_cat(&socket, session_name);

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    // Give the PTY a moment to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send input bytes
    let input = b"hello world\n";
    ws.send(Message::Binary(input.to_vec().into()))
        .await
        .expect("send should succeed");

    // Read output until we see our input echoed back (cat echoes stdin to stdout)
    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Binary(data)))) => {
                collected.push_str(&String::from_utf8_lossy(&data));
                if collected.contains("hello world") {
                    break;
                }
            }
            Ok(Some(Ok(_))) => {} // ignore non-binary frames
            _ => break,
        }
    }

    assert!(
        collected.contains("hello world"),
        "expected 'hello world' in PTY output, got: {collected:?}"
    );

    // Clean up
    let _ = ws.close(None).await;
    kill_tmux_session(&socket, session_name);
}

/// Build a raw WS upgrade request carrying an explicit `Origin`, so a test can
/// forge the header a browser would send. `host` is the daemon's real address
/// (routing), `origin` is the value under test (the guard).
fn ws_upgrade_request(
    url: &str,
    host: &str,
    origin: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(url)
        .header("Host", host)
        .header("Origin", origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap()
}

/// Assert a handshake failed with an exact HTTP 403 (`Origin not allowed`),
/// not merely "some error" — a DNS/refused/timeout error would satisfy
/// `is_err()` while proving nothing about the origin guard.
fn assert_handshake_403(
    result: Result<
        (
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::handshake::client::Response,
        ),
        tokio_tungstenite::tungstenite::Error,
    >,
) {
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status(), 403, "expected 403 Origin refusal");
        }
        Ok(_) => panic!("expected HTTP 403 handshake refusal, got a successful upgrade"),
        Err(other) => panic!("expected HTTP 403 handshake refusal, got {other:?}"),
    }
}

/// Layer 3a: WS /sessions/<id>/pty rejects requests with bad Origin header.
#[tokio::test]
async fn pty_ws_rejects_bad_origin() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): a configured origin is accepted on the PTY upgrade. The
/// session doesn't exist, so the bridge collapses right after — but the
/// handshake has already returned 101, which is all this asserts (no I/O).
#[tokio::test]
async fn pty_ws_accepts_configured_origin() {
    let origin = "http://pdo.example:9999";
    let daemon =
        TestDaemon::spawn_with_allowed_ws_origins(|_repo| Ok(()), vec![origin.to_string()])
            .await
            .unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), origin);

    let (_ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("configured origin should complete the WS handshake");
    assert_eq!(resp.status(), 101, "configured origin should upgrade");
}

/// Layer 3a (#564): with an allowlist set, an origin NOT on it is still 403 on
/// the PTY upgrade.
#[tokio::test]
async fn pty_ws_rejects_origin_absent_from_allowlist() {
    let daemon = TestDaemon::spawn_with_allowed_ws_origins(
        |_repo| Ok(()),
        vec!["https://pdo.example.com".to_string()],
    )
    .await
    .unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): `/ws` (dashboard event stream) rejects a bad Origin. This is
/// the one endpoint whose behaviour CHANGES — it had no guard before — so it had
/// zero coverage until now.
#[tokio::test]
async fn ws_events_rejects_bad_origin() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): a configured origin is accepted on `/ws`, AND the event
/// stream still works — a bare 101 wouldn't prove the stream survived, so read
/// the first frame and assert the `{"type":"ready"}` handshake.
#[tokio::test]
async fn ws_events_accepts_configured_origin() {
    let origin = "https://pdo.example.com";
    let daemon =
        TestDaemon::spawn_with_allowed_ws_origins(|_repo| Ok(()), vec![origin.to_string()])
            .await
            .unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), origin);

    let (mut ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("configured origin should complete the /ws handshake");
    assert_eq!(resp.status(), 101, "configured origin should upgrade");

    let msg = ws
        .next()
        .await
        .expect("stream should yield a first frame")
        .expect("first frame should not be an error");
    let text = msg.into_text().expect("first frame should be text");
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("first frame should be JSON");
    assert_eq!(
        parsed["type"], "ready",
        "the event stream should still open with a ready frame"
    );
}

/// Layer 3a (#564): the localhost default keeps working when a third-party
/// allowlist is set (additive, D2) — the layer-3b / Playwright guard-rail. The
/// port is ephemeral, so the "default" origin is built from the bound addr,
/// never a literal, and the allowlist entry is deliberately unrelated to it.
#[tokio::test]
async fn ws_default_localhost_origin_still_allowed_with_allowlist_set() {
    let daemon = TestDaemon::spawn_with_allowed_ws_origins(
        |_repo| Ok(()),
        vec!["https://pdo.example.com".to_string()],
    )
    .await
    .unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let default_origin = format!("http://127.0.0.1:{}", daemon.addr.port());
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), &default_origin);

    let (_ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("loopback origin should still upgrade with an allowlist set");
    assert_eq!(resp.status(), 101, "loopback default must remain allowed");
}

// --- #495: the PTY bridge must reap its `tmux attach` child on WS close ---

/// Parse `/proc/<pid>/stat` into `(comm, state, ppid)`. The comm field is
/// wrapped in parens and may itself contain spaces or parens, so split on the
/// LAST `)` rather than tokenising the whole line.
#[cfg(target_os = "linux")]
fn read_proc_stat(pid: u32) -> Option<(String, char, u32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat[open + 1..close].to_string();
    let mut fields = stat[close + 1..].split_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid: u32 = fields.next()?.parse().ok()?;
    Some((comm, state, ppid))
}

/// PIDs of all top-level `/proc` entries (child processes appear here; threads
/// of our own process do not — they live under `/proc/<pid>/task/`).
#[cfg(target_os = "linux")]
fn proc_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .collect()
}

/// Count live `tmux attach` clients that are children of `me` and whose command
/// line mentions `session_name`. The session name is unique per test, so this
/// never collides with a sibling `#[test]` running in the same binary. A client
/// that was spawned but never reaped shows up here (pre-#495 it stays ALIVE,
/// because the reader task keeps a dup of the master fd open, so no SIGHUP ever
/// reaches it).
#[cfg(target_os = "linux")]
fn attach_children(me: u32, session_name: &str) -> usize {
    proc_pids()
        .into_iter()
        .filter(|&pid| match read_proc_stat(pid) {
            Some((_comm, _state, ppid)) if ppid == me => {
                std::fs::read(format!("/proc/{pid}/cmdline"))
                    .map(|c| String::from_utf8_lossy(&c).contains(session_name))
                    .unwrap_or(false)
            }
            _ => false,
        })
        .count()
}

/// Count zombie (`<defunct>`) `tmux` children of `me`. Zombies carry no cmdline
/// so they can't be filtered by session name — the caller measures the delta
/// against a baseline captured before acting (sibling tests in the same binary
/// share this process, cf. reaper test traps).
#[cfg(target_os = "linux")]
fn tmux_zombies(me: u32) -> usize {
    proc_pids()
        .into_iter()
        .filter(|&pid| {
            matches!(
                read_proc_stat(pid),
                Some((comm, 'Z', ppid)) if ppid == me && comm.contains("tmux")
            )
        })
        .count()
}

/// Layer 3a (#495): after the PTY WebSocket closes, the daemon must reap the
/// `tmux attach` child it spawned — leaving neither a live orphan client nor a
/// `<defunct>` zombie.
///
/// The daemon runs in-process (`serve_with_config`), so the client it forks is
/// a child of THIS test process and is observable via `/proc`. Pre-fix the
/// client stays ALIVE after close (task 1 keeps a dup of the master fd, so no
/// SIGHUP reaches it), and `attach_children` never returns to 0 — the poll
/// below times out and the test fails. That is the negative control.
///
/// Linux-only: the assertion reads `/proc`. On other platforms the test is
/// compiled out (there is no CI target for them).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn pty_ws_reaps_tmux_child_on_close() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // Out-of-band session (no run in the event log) → opt out of the orphan
    // sweep so it can't race the test and kill the session/client for us. A
    // per-daemon flag, not `set_var`: this test's own `remove_var` is what used to
    // arm the sibling echo test's reaper (cf. `TestDaemon::spawn_nested`).
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    let session_name = "pdo-pty-test-reap";
    kill_tmux_session(&socket, session_name);
    create_tmux_session_with_cat(&socket, session_name);

    let me = std::process::id();
    // Zombies are shared across sibling `#[test]`s and can't be name-filtered,
    // so assert on the delta from a pre-action baseline, not an absolute count.
    let baseline_zombies = tmux_zombies(me);

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    // Wait for the attach client to actually come up before acting — this is
    // both a positive control (proves the /proc probe sees the child) and a
    // guard against measuring the reap before the child even exists.
    let up_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while attach_children(me, session_name) == 0 && tokio::time::Instant::now() < up_deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        attach_children(me, session_name) >= 1,
        "tmux attach client never appeared as a child of the test process"
    );

    // Exchange a byte so the bridge is fully wired, then close — the close is
    // what must trigger the reap.
    ws.send(Message::Binary(b"x".to_vec().into()))
        .await
        .expect("send should succeed");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = ws.close(None).await;

    // The reap runs asynchronously after the bridge's `select!` returns; poll.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut live = attach_children(me, session_name);
    let mut zombies = tmux_zombies(me);
    while tokio::time::Instant::now() < deadline && (live > 0 || zombies > baseline_zombies) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        live = attach_children(me, session_name);
        zombies = tmux_zombies(me);
    }

    // Clean up before asserting so a failure can't leak the session.
    kill_tmux_session(&socket, session_name);

    assert_eq!(
        live, 0,
        "PTY bridge leaked a live `tmux attach` client after WS close (#495)"
    );
    assert!(
        zombies <= baseline_zombies,
        "PTY bridge leaked a `<defunct>` tmux zombie after WS close (#495): \
         {zombies} > baseline {baseline_zombies}"
    );
}
