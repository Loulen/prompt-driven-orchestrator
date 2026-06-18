//! Layer 3a — Pipeline Manager PTY bridge integration test (refs #56).
//!
//! Verifies that the daemon's PTY bridge accepts a WebSocket connection at
//! `WS /sessions/pdo-mgr-<run-id>/pty` and round-trips bytes through
//! the tmux session — same mechanism used by the inline Manager terminal
//! in the PipelineInfoPanel.

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

fn create_tmux_session(socket: &str, name: &str, cmd: &str) {
    let status = std::process::Command::new("tmux")
        .args(["-L", socket, "new-session", "-d", "-s", name, cmd])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
}

fn kill_tmux_session(socket: &str, name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["-L", socket, "kill-session", "-t", name])
        .status();
}

/// Layer 3a: manager session PTY WebSocket round-trips bytes via
/// `WS /sessions/pdo-mgr-<run-id>/pty`.
#[tokio::test]
async fn manager_pty_ws_roundtrip() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let run_id = "test-run-mgr-56";
    let session_name = format!("pdo-mgr-{run_id}");

    // Daemon must come up first so we know which socket to seed the
    // manager session on. With the per-daemon socket isolation
    // (post-#86), the daemon's PTY bridge attaches via `tmux -L
    // pdo-<port>` — `default` is a different tmux server.
    //
    // This test exercises the PTY bridge, not the reaper. The manager session
    // below is created out-of-band (its run is absent from the event log), so
    // the daemon's orphan sweep would race the test and kill it — opt out of
    // all automatic cleanup for this daemon.
    std::env::set_var("PDO_DAEMON_NO_CLEANUP", "1");
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();
    std::env::remove_var("PDO_DAEMON_NO_CLEANUP");
    let socket = daemon.tmux_socket();

    kill_tmux_session(&socket, &session_name);
    create_tmux_session(&socket, &session_name, "cat");

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect to manager PTY should succeed");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let input = b"manager-hello\n";
    ws.send(Message::Binary(input.to_vec().into()))
        .await
        .expect("send should succeed");

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
                if collected.contains("manager-hello") {
                    break;
                }
            }
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }

    assert!(
        collected.contains("manager-hello"),
        "expected 'manager-hello' in manager PTY output, got: {collected:?}"
    );

    let _ = ws.close(None).await;
    kill_tmux_session(&socket, &session_name);
}
