//! Layer 4 (ADR-0080) — the WebSockets of the daemon at their HTTP seam
//! (ADR-0083):
//!
//! - **closing**: a client that sends a Close frame on `/ws` or a terminal's
//!   PTY receives the daemon's Close frame before the TCP connection ends (a
//!   closing without it is a 1006, a failure for the browser);
//! - **journal**: an opening, a closing and a refused upgrade (Origin, not
//!   admitted) each write their line.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pdo::extension_api::Extension;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message};
use tokio_tungstenite::tungstenite::Error;

use crate::common::{flow, TestDaemon};
use crate::test_extension::gate::Gate;
use crate::test_extension::{Refusal, TestExtension};

/// What the daemon logs while a test runs, read as text.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<u8>>>);

impl Journal {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }

    /// The lines holding every one of `parts`, waiting up to 5 s for one.
    async fn lines_with(&self, parts: &[&str]) -> Vec<String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let lines: Vec<String> = self
                .text()
                .lines()
                .filter(|line| parts.iter().all(|part| line.contains(part)))
                .map(str::to_string)
                .collect();
            if !lines.is_empty() || tokio::time::Instant::now() > deadline {
                return lines;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Write for Journal {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Journal {
    type Writer = Journal;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Capture the log of this thread (the test's runtime runs the daemon on it)
/// until the guard drops.
fn capture() -> (Journal, tracing::subscriber::DefaultGuard) {
    let journal = Journal::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(journal.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (journal, guard)
}

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Send a Close (1000) and return what the daemon sends back before the end.
async fn close_and_read_answer(ws: &mut Socket) -> Option<Message> {
    ws.send(Message::Close(Some(CloseFrame {
        code: CloseCode::Normal,
        reason: "".into(),
    })))
    .await
    .expect("the Close frame is sent");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, ws.next()).await {
            Ok(Some(Ok(Message::Close(frame)))) => return Some(Message::Close(frame)),
            // What the daemon had in flight before reading the Close.
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn the_event_stream_answers_the_client_s_close() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", daemon.addr))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap(); // `ready`

    let answer = close_and_read_answer(&mut ws).await;
    assert!(
        matches!(answer, Some(Message::Close(_))),
        "the daemon answers the Close before the end, got {answer:?}"
    );
}

#[tokio::test]
async fn a_terminal_answers_the_client_s_close() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();
    let session = "pdo-ws-closing-pty";
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", session])
        .status();
    let created = std::process::Command::new("tmux")
        .args(["-L", &socket, "new-session", "-d", "-s", session, "cat"])
        .status()
        .unwrap();
    assert!(created.success());

    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{}/sessions/{session}/pty", daemon.addr))
            .await
            .unwrap();
    // The bridge is up once the PTY spoke.
    let first = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
    assert!(matches!(first, Ok(Some(Ok(_)))), "{first:?}");

    let answer = close_and_read_answer(&mut ws).await;
    let _ = std::process::Command::new("tmux")
        .args(["-L", &socket, "kill-session", "-t", session])
        .status();
    assert!(
        matches!(answer, Some(Message::Close(_))),
        "the PTY answers the Close before the end, got {answer:?}"
    );
}

#[tokio::test]
async fn an_opening_and_a_closing_have_their_lines_in_the_journal() {
    let (journal, _guard) = capture();
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", daemon.addr))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap();

    let opened = journal
        .lines_with(&["WebSocket opened: /ws", "ms to the 101"])
        .await;
    assert_eq!(opened.len(), 1, "{}", journal.text());

    close_and_read_answer(&mut ws).await;
    let closed = journal
        .lines_with(&["WebSocket closed: /ws", "by the client, code 1000, clean"])
        .await;
    assert_eq!(closed.len(), 1, "{}", journal.text());
}

#[tokio::test]
async fn an_upgrade_refused_for_its_origin_has_its_line_in_the_journal() {
    let (journal, _guard) = capture();
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();
    let mut request = format!("ws://{}/ws", daemon.addr)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    let refused = tokio_tungstenite::connect_async(request).await;
    assert!(matches!(refused, Err(Error::Http(ref r)) if r.status() == 403));

    let lines = journal
        .lines_with(&[
            "WARN",
            "WebSocket upgrade refused: /ws, 403 (Origin not allowed)",
            "Origin http://evil.example",
        ])
        .await;
    assert_eq!(lines.len(), 1, "{}", journal.text());
}

#[tokio::test]
async fn an_upgrade_nobody_admitted_has_its_line_in_the_journal() {
    let (journal, _guard) = capture();
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    control.set_gate(Gate::token("s3cret"));
    let extensions: Vec<Box<dyn Extension>> = vec![Box::new(ext)];
    let daemon = TestDaemon::spawn_with_extensions(
        |repo| flow::seed_solo(repo, &[("solo", false)]),
        extensions,
    )
    .await
    .unwrap();

    for path in ["/ws", "/sessions/pdo-none/pty"] {
        let mut request = format!("ws://{}{path}", daemon.addr)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", format!("http://{}", daemon.addr).parse().unwrap());
        let refused = tokio_tungstenite::connect_async(request).await;
        assert!(matches!(refused, Err(Error::Http(ref r)) if r.status() == 401));
        let lines = journal
            .lines_with(&[
                "WARN",
                &format!("WebSocket upgrade refused: {path}, 401 (not admitted)"),
                &format!("Origin http://{}", daemon.addr),
            ])
            .await;
        assert_eq!(lines.len(), 1, "{path}: {}", journal.text());
    }
    // A plain request refused alike is not journaled.
    let resp = reqwest::get(format!("{}/runs", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert!(
        !journal.text().contains("refused: /runs"),
        "{}",
        journal.text()
    );
}
