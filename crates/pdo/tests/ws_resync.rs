//! Layer 4 (ADR-0080) — ADR-0083 §6: a `/ws` client that fell behind the bus
//! is told to `resync`.
//!
//! The bus keeps a bounded backlog per subscriber; a client that has not read
//! it in time loses the oldest messages, and with them maybe a Run's
//! transition. Nothing is replayed: the daemon sends `{"type":"resync"}`,
//! which the interface answers with a full re-read.

use std::time::Duration;

use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::common::TestDaemon;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next Text frame, as JSON, before `deadline`.
async fn next_json(ws: &mut Socket, deadline: tokio::time::Instant) -> Option<serde_json::Value> {
    loop {
        match tokio::time::timeout_at(deadline, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return serde_json::from_str(&text).ok(),
            Ok(Some(Ok(_))) => {}
            _ => return None,
        }
    }
}

#[tokio::test]
async fn a_lagging_client_is_told_to_resync() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let mut ws = daemon.connect_ws().await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    assert_eq!(
        next_json(&mut ws, deadline).await.expect("ready")["type"],
        "ready"
    );

    // Far more than the bus keeps, in one synchronous burst: the relay cannot
    // read any of it in between, so it lags.
    daemon.flood_bus(5_000);

    let mut resync = false;
    while let Some(msg) = next_json(&mut ws, deadline).await {
        if msg["type"] == "resync" {
            resync = true;
            break;
        }
    }
    assert!(resync, "a lagging client must be told to resync");
}

#[tokio::test]
async fn a_client_that_keeps_up_is_never_told_to_resync() {
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let mut ws = daemon.connect_ws().await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    assert_eq!(
        next_json(&mut ws, deadline).await.expect("ready")["type"],
        "ready"
    );

    // Fewer than the bus keeps: nothing is lost, nothing to re-read.
    daemon.flood_bus(3);

    let mut seen = 0;
    while seen < 3 {
        let msg = next_json(&mut ws, deadline).await.expect("flood message");
        assert_ne!(msg["type"], "resync", "{msg}");
        if msg["type"] == "test_flood" {
            seen += 1;
        }
    }
}
