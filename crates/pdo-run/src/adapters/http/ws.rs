//! `/ws`: the live stream every client subscribes to. It relays the kernel's
//! bus as is (D20), after a `ready` greeting, with a heartbeat every
//! [`HEARTBEAT_INTERVAL`].

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use pdo_session::adapters::http::origin::{check_origin, origin_for_log};
use tokio::sync::broadcast;
use tokio::time;
use tracing::{info, warn};

use crate::Runs;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

pub(super) async fn upgrade(
    State(runs): State<Arc<Runs>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // `/ws` streams every event of every Run to any subscriber, so without this guard
    // any web page open in the operator's browser could subscribe and exfiltrate
    // repos/prompts/verdicts. `headers` MUST precede `ws`: the upgrade extractor
    // consumes the request, so it stays last (axum 0.8).
    let options = runs.options();
    if !check_origin(&headers, options.daemon_port, &options.allowed_ws_origins) {
        warn!(
            "Rejected /ws WebSocket upgrade: Origin not allowed ({})",
            origin_for_log(&headers)
        );
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    ws.on_upgrade(move |socket| relay(socket, runs))
}

async fn relay(mut socket: WebSocket, runs: Arc<Runs>) {
    info!("WebSocket client connected");
    let ready = serde_json::json!({ "type": "ready" });
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut bus = runs.bus().subscribe();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let msg = serde_json::json!({
                    "type": "heartbeat",
                    "ts": unix_timestamp_secs(),
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            message = bus.recv() => {
                match message {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    info!("WebSocket client disconnected");
}

fn unix_timestamp_secs() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    format!("{secs}.{millis:03}")
}
