//! Shared test harness for Cargo integration tests (testing pyramid layer 3a).
//!
//! Boots a real daemon on an ephemeral port over a `tempfile::TempDir`. No mocking
//! of notify, sqlite, or axum — that's the whole point of layer 3a per ADR 0004.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Result;
use maestro_daemon::{serve, DaemonHandle};
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub struct TestDaemon {
    pub addr: SocketAddr,
    tempdir: TempDir,
    handle: Option<DaemonHandle>,
}

impl TestDaemon {
    /// Spawn a fresh daemon backed by a tempdir. The `setup` callback receives the
    /// tempdir path and may seed it (write yaml, init a git repo, etc.) before the
    /// daemon starts.
    pub async fn spawn<F>(setup: F) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        // When the test suite itself runs inside a Maestro node (e.g. an agent
        // worktree), `MAESTRO_NODE_ID` is exported in the environment and the
        // daemon under test would consider itself "nested" — silently disabling
        // the orphan sweep and reaper, and failing every test that asserts on
        // them. A TestDaemon must behave like a top-level daemon regardless of
        // where the tests run; nested-mode tests opt back in explicitly via
        // `MAESTRO_DAEMON_NO_CLEANUP=1`.
        std::env::remove_var("MAESTRO_NODE_ID");

        let tempdir = tempfile::tempdir()?;
        setup(tempdir.path())?;

        let handle = serve(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            tempdir.path().to_path_buf(),
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn repo_root(&self) -> &Path {
        self.tempdir.path()
    }

    /// Tmux socket scoped to this daemon (`tmux -L <name>`). Tests that
    /// spawn or inspect tmux sessions out-of-band must use this socket so
    /// they hit the same tmux server the daemon talks to.
    pub fn tmux_socket(&self) -> String {
        maestro_daemon::tmux_session_manager::tmux_socket_name(self.addr.port())
    }

    /// Drive a single Trigger scheduler tick synchronously (test seam).
    pub async fn run_trigger_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_trigger_tick().await;
        }
    }

    /// Force a Trigger's next fire into the past so the next tick treats it as
    /// due (test seam).
    pub async fn force_trigger_due(&self, trigger_id: &str) {
        if let Some(handle) = self.handle.as_ref() {
            handle.force_trigger_due(trigger_id).await;
        }
    }

    /// Open a WebSocket connection to `/ws`. Returns the connected stream so the
    /// test can read the initial `{"type":"ready"}` and any subsequent events.
    pub async fn connect_ws(&self) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
        let url = format!("ws://{}/ws", self.addr);
        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
        Ok(ws)
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.task.abort();
        }
    }
}

#[allow(dead_code)]
pub fn ws_text(msg: &Message) -> Option<&str> {
    match msg {
        Message::Text(s) => Some(s.as_str()),
        _ => None,
    }
}
