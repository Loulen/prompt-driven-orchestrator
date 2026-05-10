//! PTY bridge: spawns `tmux attach -t <session>` inside a pseudo-terminal and
//! bridges byte I/O between the PTY and a WebSocket connection.
//!
//! Protocol (WS → daemon):
//! - Binary frames → stdin of the PTY (user keystrokes)
//! - Text frames with JSON `{"type":"resize","cols":N,"rows":N}` → PTY resize
//!
//! Protocol (daemon → WS):
//! - Binary frames ← stdout of the PTY (terminal output)

use std::io::{Read, Write};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ws::WebSocketUpgrade, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Validate the Origin header against the daemon's own address to prevent
/// DNS-rebinding attacks. Returns `true` if the origin is acceptable.
pub fn check_origin(headers: &HeaderMap, daemon_port: u16) -> bool {
    // No Origin header — e.g. same-origin requests, curl, or non-browser
    // clients. Allow these; the browser always sends Origin on WS upgrade.
    let Some(origin_header) = headers.get("origin") else {
        return true;
    };
    let Ok(origin) = origin_header.to_str() else {
        return false;
    };

    let origin_lower = origin.to_lowercase();

    let allowed = [
        format!("http://localhost:{daemon_port}"),
        format!("http://127.0.0.1:{daemon_port}"),
        format!("https://localhost:{daemon_port}"),
        format!("https://127.0.0.1:{daemon_port}"),
    ];

    allowed.contains(&origin_lower)
}

/// A resize message sent from the xterm.js client.
#[derive(Debug, Deserialize, PartialEq)]
pub struct ResizeMsg {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub cols: u16,
    pub rows: u16,
}

/// Try to decode a text WS frame as a resize message.
pub fn decode_resize(text: &str) -> Option<ResizeMsg> {
    let msg: ResizeMsg = serde_json::from_str(text).ok()?;
    if msg.msg_type == "resize" && msg.cols > 0 && msg.rows > 0 {
        Some(msg)
    } else {
        None
    }
}

/// Axum handler for `WS /sessions/{session_id}/pty`.
pub(crate) async fn session_pty_handler(
    AxumPath(session_id): AxumPath<String>,
    State(state): State<Arc<super::AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !check_origin(&headers, state.port) {
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }

    let tmux_socket = state.tmux_socket();
    ws.on_upgrade(move |socket| handle_pty_ws(socket, tmux_socket, session_id))
}

async fn handle_pty_ws(socket: WebSocket, tmux_socket: String, session_id: String) {
    info!("PTY WebSocket opened for session {session_id}");

    let pty_system = native_pty_system();
    let initial_size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = match pty_system.openpty(initial_size) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to open PTY: {e}");
            return;
        }
    };

    let mut cmd = CommandBuilder::new("tmux");
    // Pin the attach to the daemon's private socket so we don't accidentally
    // reach into another maestro daemon's tmux state on the same host.
    cmd.args(["-L", tmux_socket.as_str(), "attach", "-t", &session_id]);

    let _child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn tmux attach for {session_id}: {e}");
            return;
        }
    };

    drop(pair.slave);

    let mut pty_reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to clone PTY reader: {e}");
            return;
        }
    };
    let mut pty_writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to take PTY writer: {e}");
            return;
        }
    };

    let master = pair.master;
    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel: PTY stdout → async sender → WebSocket
    let (pty_tx, mut pty_rx) = mpsc::channel::<Vec<u8>>(64);

    // Task 1: blocking read from PTY, send chunks through channel
    let read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pty_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Task 2: forward PTY output from channel to WebSocket
    let ws_send_handle = tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            if ws_sink.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // Task 3: read from WebSocket, write to PTY stdin (+ handle resize)
    let ws_recv_handle = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) if pty_writer.write_all(&data).is_err() => {
                    break;
                }
                Message::Binary(_) => {}
                Message::Text(text) => {
                    if let Some(resize) = decode_resize(&text) {
                        let new_size = PtySize {
                            rows: resize.rows,
                            cols: resize.cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        };
                        if let Err(e) = master.resize(new_size) {
                            warn!("PTY resize failed: {e}");
                        }
                    } else {
                        // Text that isn't a control message — treat as input
                        if pty_writer.write_all(text.as_bytes()).is_err() {
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for any task to finish, then clean up
    tokio::select! {
        _ = read_handle => {}
        _ = ws_send_handle => {}
        _ = ws_recv_handle => {}
    }

    info!("PTY WebSocket closed for session {session_id}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    // --- Origin check tests ---

    #[test]
    fn origin_check_allows_localhost_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:5172"));
        assert!(check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_allows_127_0_0_1_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:5172"));
        assert!(check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_rejects_wrong_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:9999"));
        assert!(!check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_rejects_external_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://evil.com"));
        assert!(!check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_allows_no_origin_header() {
        let headers = HeaderMap::new();
        assert!(check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_allows_https_localhost() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("https://localhost:5172"));
        assert!(check_origin(&headers, 5172));
    }

    #[test]
    fn origin_check_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("HTTP://LOCALHOST:5172"));
        assert!(check_origin(&headers, 5172));
    }

    // --- Resize message decoder tests ---

    #[test]
    fn decode_resize_valid() {
        let msg = decode_resize(r#"{"type":"resize","cols":120,"rows":40}"#);
        assert_eq!(
            msg,
            Some(ResizeMsg {
                msg_type: "resize".into(),
                cols: 120,
                rows: 40,
            })
        );
    }

    #[test]
    fn decode_resize_rejects_zero_cols() {
        assert_eq!(
            decode_resize(r#"{"type":"resize","cols":0,"rows":40}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_zero_rows() {
        assert_eq!(
            decode_resize(r#"{"type":"resize","cols":80,"rows":0}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_wrong_type() {
        assert_eq!(
            decode_resize(r#"{"type":"data","cols":80,"rows":24}"#),
            None
        );
    }

    #[test]
    fn decode_resize_rejects_garbage() {
        assert_eq!(decode_resize("not json at all"), None);
    }

    #[test]
    fn decode_resize_rejects_missing_fields() {
        assert_eq!(decode_resize(r#"{"type":"resize"}"#), None);
    }
}
