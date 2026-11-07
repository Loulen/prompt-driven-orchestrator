//! `/ws`: the live stream every client subscribes to. It relays the kernel's
//! bus as is (D20), after a `ready` greeting, with a heartbeat every
//! [`HEARTBEAT_INTERVAL`]. A connection that lagged behind the bus (messages
//! lost) receives [`RESYNC`] (ADR-0083 §6).
//!
//! Filtered per connection (CORE-5, `ws_visibility`): a message about a Run
//! reaches a connection only if its Actor (the one `identify` set on the
//! upgrade request) may read that Run; a message guarded by a read only if it
//! may run that read. The other messages pass as before.
//!
//! The client says which Run its tab shows (`{"type": "show_run", "run_id":
//! <id> | null}`, [`SHOW_RUN`]): the Presences on the Runs (API 0.12). The
//! connection's end is its departure.
//!
//! The life of a connection (API 0.23): the client names its tab on the
//! upgrade (`/ws?tab=<id>`), and a new connection of that tab replaces the
//! one it still had open, closed with [`REPLACED`]. The daemon pings every
//! connection ([`StreamLiveness`]); one it heard nothing from (no Pong, no
//! message) beyond the delay is mute and dropped, as is one whose writes
//! stay stuck that long. Either way its Presence falls.
//!
//! The client's Close frame is answered before the connection ends (ADR-0083
//! §3): a closing without it counts as a failure for the browser. Every
//! opening, closing and refused upgrade has its line in the journal
//! ([`ws_log`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use futures_util::SinkExt;
use pdo_kernel::Actor;
use pdo_session::adapters::http::origin::check_origin;
use pdo_session::adapters::http::ws_log::{self, Closing, NO_STATUS};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time;
use tracing::warn;

use super::ws_visibility::Visibility;
use crate::application::presence::StreamConnection;
use crate::domain::stream_liveness::{Liveness, StreamLiveness};
use crate::Runs;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// What a client that lagged behind the bus receives: re-read the whole state.
pub const RESYNC: &str = r#"{"type":"resync"}"#;

/// What a client sends when its tab shows another Run, or none.
pub const SHOW_RUN: &str = "show_run";

/// The close code of a connection a newer connection of its tab replaced
/// (a private code, RFC 6455 §7.4.2).
pub const REPLACED: u16 = 4000;

/// How long the Close of a replaced connection may take: it is likely dead.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// The path of the stream, as the journal names it.
const PATH: &str = "/ws";

/// The upgrade's query: the tab the connection comes from.
#[derive(Debug, Default, Deserialize)]
pub(super) struct StreamQuery {
    tab: Option<String>,
}

pub(super) async fn upgrade(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // `/ws` streams every event of every Run to any subscriber, so without this guard
    // any web page open in the operator's browser could subscribe and exfiltrate
    // repos/prompts/verdicts. `headers` MUST precede `ws`: the upgrade extractor
    // consumes the request, so it stays last (axum 0.8).
    let requested = Instant::now();
    let options = runs.options();
    if !check_origin(&headers, options.daemon_port, &options.allowed_ws_origins) {
        ws_log::refused(PATH, StatusCode::FORBIDDEN, "Origin not allowed", &headers);
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    ws.on_upgrade(move |socket| async move {
        ws_log::opened(PATH, requested.elapsed());
        let opened = Instant::now();
        let connection = runs.open_connection(&actor, query.tab.as_deref());
        let liveness = runs.stream_liveness();
        let closing = relay(socket, runs.clone(), actor, &connection, liveness).await;
        runs.close_connection(connection.id);
        ws_log::closed(PATH, closing, opened.elapsed());
    })
}

/// Relay the bus to `socket` until either side ends it, `replaced` tells it a
/// newer connection of its tab took over, or it goes mute; how it ended.
async fn relay(
    mut socket: WebSocket,
    runs: Arc<Runs>,
    actor: Actor,
    connection: &StreamConnection,
    timings: StreamLiveness,
) -> Closing {
    let limit = timings.mute_after;
    let ready = serde_json::json!({ "type": "ready" });
    if let Err(closing) = send(&mut socket, text(&ready), limit).await {
        return closing;
    }

    let mut bus = runs.bus().subscribe();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    let mut ping = time::interval(timings.ping_every);
    let mut liveness = Liveness::new(timings.mute_after, Instant::now());
    let mut visible = Visibility::new(&runs, actor.clone());

    loop {
        tokio::select! {
            _ = connection.replaced.notified() => {
                let frame = CloseFrame { code: REPLACED, reason: "replaced".into() };
                let _ = time::timeout(CLOSE_GRACE, socket.send(Message::Close(Some(frame)))).await;
                return Closing::Replaced;
            }
            _ = ping.tick() => {
                if liveness.is_mute(Instant::now()) {
                    return Closing::Mute(timings.mute_after);
                }
                if let Err(closing) = send(&mut socket, Message::Ping(Default::default()), limit).await {
                    return closing;
                }
            }
            _ = heartbeat.tick() => {
                visible.forget_old();
                let msg = serde_json::json!({
                    "type": "heartbeat",
                    "ts": unix_timestamp_secs(),
                });
                if let Err(closing) = send(&mut socket, text(&msg), limit).await {
                    return closing;
                }
            }
            incoming = socket.recv() => {
                if matches!(incoming, Some(Ok(_))) {
                    liveness.heard(Instant::now());
                }
                match incoming {
                    Some(Ok(Message::Close(frame))) => {
                        // The answer is queued by the protocol; flushing sends
                        // it before the connection is dropped.
                        let _ = socket.flush().await;
                        return Closing::ByClient(frame.map_or(NO_STATUS, |f| f.code));
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Some(run_id) = shown_run(&text) {
                            runs.show_run(connection.id, &actor, run_id.as_deref()).await;
                        }
                    }
                    // Pings are answered by the protocol itself; a Pong only
                    // says the connection lives.
                    Some(Ok(_)) => {}
                    None | Some(Err(_)) => return Closing::Dropped,
                }
            }
            message = bus.recv() => {
                match message {
                    Ok(msg) => {
                        let Some(msg) = visible.filter(msg).await else {
                            continue;
                        };
                        if let Err(closing) = send(&mut socket, text(&msg), limit).await {
                            return closing;
                        }
                    }
                    // ADR-0083 §6: the bus dropped messages this client never
                    // got, so its picture may miss a transition. Nothing is
                    // replayed: the client is told to re-read everything.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {n} events, asking it to resync");
                        if let Err(closing) = send(&mut socket, Message::Text(RESYNC.into()), limit).await {
                            return closing;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let _ = socket.close().await;
                        return Closing::ByDaemon;
                    }
                }
            }
        }
    }
}

/// `message` as a text frame.
fn text(message: &Value) -> Message {
    Message::Text(message.to_string().into())
}

/// Send `message`, within `limit`: a write stuck that long (a peer that reads
/// nothing any more) is a mute connection.
async fn send(socket: &mut WebSocket, message: Message, limit: Duration) -> Result<(), Closing> {
    match time::timeout(limit, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(Closing::Dropped),
        Err(_) => Err(Closing::Mute(limit)),
    }
}

/// The Run a client's message says its tab shows (`Some(None)`: none), or
/// `None` for any other message.
fn shown_run(text: &str) -> Option<Option<String>> {
    let message: Value = serde_json::from_str(text).ok()?;
    if message["type"] != SHOW_RUN {
        return None;
    }
    Some(message["run_id"].as_str().map(str::to_string))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_says_which_run_its_tab_shows() {
        assert_eq!(
            shown_run(r#"{"type":"show_run","run_id":"r1"}"#),
            Some(Some("r1".to_string()))
        );
        assert_eq!(
            shown_run(r#"{"type":"show_run","run_id":null}"#),
            Some(None)
        );
        assert_eq!(shown_run(r#"{"type":"show_run"}"#), Some(None));
        assert_eq!(shown_run(r#"{"type":"hello"}"#), None);
        assert_eq!(shown_run("not json"), None);
    }
}
