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

/// Env var that EXTENDS the WebSocket Origin allowlist with operator-supplied
/// origins (#564). Comma-separated exact origins (`scheme://host[:port]`), read
/// once at boot in [`super::DaemonConfig::from_env`] — never in the hot path
/// (#181/#407). Additive: the four localhost/127.0.0.1 defaults always stay.
pub(crate) const ALLOWED_WS_ORIGINS_ENV: &str = "PDO_ALLOWED_WS_ORIGINS";

/// Parse the raw [`ALLOWED_WS_ORIGINS_ENV`] value into a normalised, ready-to-
/// compare list. **Pure** (takes the raw string, touches no env), so the truth
/// table is testable without a process-global env (#181).
///
/// Split on `,` → trim → strip **one** trailing `/` → lowercase → drop empties.
/// A browser Origin never carries a path or trailing slash (RFC 6454 §7.1:
/// `scheme "://" host [ ":" port ]`), so stripping a single `/` forgives an
/// operator who pastes `https://pdo.example.tld/` at zero cost. We deliberately
/// do NOT use `trim_end_matches('/')`: eating several slashes would silently
/// "repair" a malformed `…//` entry into something that matches, hiding a typo.
/// No wildcard handling (D3): `*` survives as a literal that can never equal a
/// real Origin, so mis-configuring it opens nothing.
pub(crate) fn parse_allowed_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|entry| {
            let entry = entry.trim();
            let entry = entry.strip_suffix('/').unwrap_or(entry);
            entry.to_lowercase()
        })
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// Validate the Origin header against the daemon's own address, plus any
/// operator-configured `extra_origins`, to prevent DNS-rebinding / cross-site
/// WebSocket hijacking (CSWSH). Returns `true` if the origin is acceptable.
///
/// Shared by BOTH WebSocket upgrades — `WS /sessions/{id}/pty` (the terminal)
/// and `WS /ws` (the dashboard event stream, #564). It lives here next to
/// [`parse_allowed_origins`] on purpose: both sides normalise to lowercase and
/// the pair only reads sensibly together. `extra_origins` are the already-parsed,
/// already-lowercased entries from [`ALLOWED_WS_ORIGINS_ENV`]; they ADD to the
/// four localhost defaults, never replace them (D2).
pub(crate) fn check_origin(
    headers: &HeaderMap,
    daemon_port: u16,
    extra_origins: &[String],
) -> bool {
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

    allowed.contains(&origin_lower) || extra_origins.contains(&origin_lower)
}

/// Render the received Origin header for a rejection log line (#564, D8),
/// truncated so a hostile client can't flood the log with a giant "origin".
/// `<none>` when absent, `<non-utf8>` when the value isn't visible-ASCII — the
/// same two branches [`check_origin`] treats specially.
pub(crate) fn origin_for_log(headers: &HeaderMap) -> String {
    const MAX_CHARS: usize = 256;
    match headers.get("origin") {
        None => "<none>".to_string(),
        Some(value) => match value.to_str() {
            Ok(s) if s.chars().count() > MAX_CHARS => {
                let truncated: String = s.chars().take(MAX_CHARS).collect();
                format!("{truncated}…")
            }
            Ok(s) => s.to_string(),
            Err(_) => "<non-utf8>".to_string(),
        },
    }
}

/// A resize message sent from the xterm.js client.
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ResizeMsg {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub cols: u16,
    pub rows: u16,
}

/// Try to decode a text WS frame as a resize message.
pub(crate) fn decode_resize(text: &str) -> Option<ResizeMsg> {
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
    if !check_origin(&headers, state.port, &state.allowed_ws_origins) {
        // Muted 403s were undiagnosable: once an operator configures a public
        // domain, a typo in PDO_ALLOWED_WS_ORIGINS is the #1 cause of a dead
        // terminal (#564, D8). warn! (not error!) — this is nominal protective
        // behaviour, and it is remotely triggerable.
        warn!(
            "Rejected PTY WebSocket upgrade: Origin not allowed ({})",
            origin_for_log(&headers)
        );
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
    // reach into another pdo daemon's tmux state on the same host.
    cmd.args(["-L", tmux_socket.as_str(), "attach", "-t", &session_id]);
    // The consumer at the other end of this PTY is xterm.js in the browser, so
    // declare that terminal type explicitly instead of inheriting the daemon's
    // ambient TERM. A daemon started headless (systemd, container, CI, nohup)
    // has TERM unset or `dumb`, which makes `tmux attach` abort with
    // "open terminal failed: terminal does not support clear" — the inline
    // manager/node terminal then shows that error instead of the session.
    cmd.env("TERM", "xterm-256color");

    // Acquire the master-side reader and writer BEFORE spawning the child.
    // Both are fallible; taking them first means that once the child exists the
    // only path out of this function is the select!/reap below — no early
    // return can drop an unreaped child (#495).
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

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn tmux attach for {session_id}: {e}");
            return;
        }
    };

    drop(pair.slave);

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
                        // Unknown / malformed control frames must NEVER be
                        // written to the PTY as user input — that would inject
                        // stray characters into whatever has the focus inside
                        // tmux (Claude Code's prompt, an editor, etc.). User
                        // keystrokes always travel as Binary frames; Text
                        // frames are reserved for our JSON control protocol.
                        warn!(
                            "Ignoring unrecognized text frame on PTY socket \
                             ({} bytes)",
                            text.len()
                        );
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

    // Reap the `tmux attach` child (#495). portable_pty's Child, like std's,
    // does NOT wait() on drop, so before this every closed pane leaked a
    // `[tmux: client] <defunct>` for the daemon's whole lifetime. A bare wait()
    // here is NOT enough: try_clone_reader()/take_writer() each dup() the master
    // fd, so on a plain socket close the reader task still holds one open, the
    // client has NOT received SIGHUP, and wait() would block forever. kill()
    // signals the client PID directly (SIGHUP, then SIGKILL after a ~250ms
    // grace) regardless of fd state; wait() then reaps it. Killing the client
    // also closes the slave, unblocking the reader task. Run on a blocking
    // thread so the grace period can't stall the async runtime.
    let _ = tokio::task::spawn_blocking(move || {
        let _ = child.kill();
        let _ = child.wait();
    })
    .await;

    info!("PTY WebSocket closed for session {session_id}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    // --- Origin check tests ---

    // The 3rd argument is the parsed `PDO_ALLOWED_WS_ORIGINS` list. An empty
    // slice is the unconfigured production/test default — the seven tests below
    // pin the localhost-only behaviour that #564 must leave untouched.

    #[test]
    fn origin_check_allows_localhost_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_127_0_0_1_on_correct_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_rejects_wrong_port() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:9999"));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_rejects_external_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://evil.com"));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_no_origin_header() {
        let headers = HeaderMap::new();
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_allows_https_localhost() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("https://localhost:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    #[test]
    fn origin_check_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("HTTP://LOCALHOST:5172"));
        assert!(check_origin(&headers, 5172, &[]));
    }

    // --- #564: configurable allowlist (PDO_ALLOWED_WS_ORIGINS) ---

    #[test]
    fn origin_check_allows_configured_origin() {
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("https://pdo.example.com"),
        );
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_still_allows_localhost_when_allowlist_configured() {
        // The single most important guard (D2): configuring an extra origin must
        // NOT drop the localhost defaults, or Playwright / layer-3 (baseURL
        // 127.0.0.1) and loopback debugging break.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:5172"));
        assert!(check_origin(&headers, 5172, &extra));
        headers.insert("origin", HeaderValue::from_static("http://localhost:5172"));
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_rejects_origin_absent_from_allowlist() {
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("https://evil.example.org"),
        );
        assert!(!check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_configured_match_is_case_insensitive() {
        // Both sides normalise to lowercase, so a shouting browser still matches
        // a lowercase allowlist entry.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            HeaderValue::from_static("HTTPS://PDO.EXAMPLE.COM"),
        );
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_configured_origin_is_scheme_and_port_exact() {
        // Exact-match only (D3): same host with a different scheme or an explicit
        // port is a different origin and must be rejected.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://pdo.example.com"));
        assert!(!check_origin(&headers, 5172, &extra));
        headers.insert(
            "origin",
            HeaderValue::from_static("https://pdo.example.com:8443"),
        );
        assert!(!check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_allows_no_origin_header_even_with_allowlist() {
        // Pins the D5 branch: a configured allowlist does not harden the
        // "no Origin ⇒ allowed" path (curl / CLI / layer-3 stay green).
        let extra = parse_allowed_origins("https://pdo.example.com");
        let headers = HeaderMap::new();
        assert!(check_origin(&headers, 5172, &extra));
    }

    #[test]
    fn origin_check_rejects_non_ascii_origin_header() {
        // A non-visible-ASCII value makes `to_str()` fail — that branch was
        // previously uncovered. Rejected with or without an allowlist.
        let extra = parse_allowed_origins("https://pdo.example.com");
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap());
        assert!(!check_origin(&headers, 5172, &extra));
        assert!(!check_origin(&headers, 5172, &[]));
    }

    // --- #564: parse_allowed_origins (pure) ---

    #[test]
    fn parse_allowed_origins_trims_lowercases_and_drops_empties() {
        let parsed = parse_allowed_origins("  https://PDO.Example.com , ,https://Other.tld ");
        assert_eq!(parsed, vec!["https://pdo.example.com", "https://other.tld"]);
    }

    #[test]
    fn parse_allowed_origins_strips_exactly_one_trailing_slash() {
        assert_eq!(
            parse_allowed_origins("https://pdo.example.com/"),
            vec!["https://pdo.example.com"]
        );
        // Only ONE slash is stripped: a malformed `…//` keeps the extra slash and
        // so never matches a real browser Origin, rather than being "repaired".
        assert_eq!(
            parse_allowed_origins("https://pdo.example.com//"),
            vec!["https://pdo.example.com/"]
        );
    }

    #[test]
    fn parse_allowed_origins_empty_or_blank_is_empty_vec() {
        assert!(parse_allowed_origins("").is_empty());
        assert!(parse_allowed_origins("   ").is_empty());
        assert!(parse_allowed_origins(",, ,").is_empty());
    }

    #[test]
    fn parse_allowed_origins_keeps_wildcard_verbatim_so_it_never_matches() {
        // No wildcard support in v1 (D3): '*' is kept as a literal entry that can
        // never equal a real Origin — mis-configuring it silently allows nothing.
        let parsed = parse_allowed_origins("*");
        assert_eq!(parsed, vec!["*"]);
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://evil.com"));
        assert!(!check_origin(&headers, 5172, &parsed));
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
