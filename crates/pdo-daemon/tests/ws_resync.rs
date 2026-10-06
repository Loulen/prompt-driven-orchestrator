//! #972 — a `/ws` client that fell behind the broadcast is told to `resync`.
//!
//! The broadcast channel keeps a bounded backlog per subscriber; when a client
//! has not drained it in time, the daemon used to log the loss and carry on, so
//! the client silently missed a Run transition. It now sends `{"type":"resync"}`,
//! which the UI answers with a full re-read.

use std::time::Duration;

use crate::common::{ws_text, TestDaemon};
use futures_util::StreamExt;

async fn next_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    deadline: tokio::time::Instant,
) -> Option<serde_json::Value> {
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(msg))) => {
                if let Some(text) = ws_text(&msg) {
                    return serde_json::from_str(text).ok();
                }
            }
            _ => return None,
        }
    }
}

#[tokio::test]
async fn a_lagging_ws_client_receives_resync() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let mut ws = daemon.connect_ws().await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let ready = next_text(&mut ws, deadline).await.expect("ready");
    assert_eq!(ready["type"], "ready");

    // Far more than the channel holds, in one synchronous burst: the socket task
    // cannot drain any of it in between, so it lags.
    daemon.flood_ws_broadcast(5_000);

    let mut saw_resync = false;
    while let Some(msg) = next_text(&mut ws, deadline).await {
        if msg["type"] == "resync" {
            saw_resync = true;
            break;
        }
    }
    assert!(saw_resync, "a lagging client must be told to resync");
}

#[tokio::test]
async fn a_client_that_keeps_up_never_receives_resync() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let mut ws = daemon.connect_ws().await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    assert_eq!(
        next_text(&mut ws, deadline).await.expect("ready")["type"],
        "ready"
    );

    // Fewer than the backlog holds: nothing is lost, nothing to resync.
    daemon.flood_ws_broadcast(3);

    let mut seen = 0;
    while seen < 3 {
        let msg = next_text(&mut ws, deadline).await.expect("flood message");
        assert_ne!(msg["type"], "resync", "{msg}");
        if msg["type"] == "test_flood" {
            seen += 1;
        }
    }
}
