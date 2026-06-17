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
    // created out-of-band (no run in the event log), so the daemon's orphan
    // sweep would race the test and kill it as unrecognised — opt out of all
    // automatic cleanup for this daemon.
    std::env::set_var("PDO_DAEMON_NO_CLEANUP", "1");
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();
    std::env::remove_var("PDO_DAEMON_NO_CLEANUP");
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

/// Layer 3a: WS /sessions/<id>/pty rejects requests with bad Origin header.
#[tokio::test]
async fn pty_ws_rejects_bad_origin() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);

    // Build a request with a malicious origin
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Host", format!("{}", daemon.addr))
        .header("Origin", "http://evil.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap();

    let result = tokio_tungstenite::connect_async(request).await;
    assert!(
        result.is_err(),
        "WS connect with bad origin should fail (403)"
    );
}
