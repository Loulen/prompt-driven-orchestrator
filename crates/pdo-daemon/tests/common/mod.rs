//! Shared test harness for Cargo integration tests (testing pyramid layer 3a).
//!
//! Boots a real daemon on an ephemeral port over a `tempfile::TempDir`. No mocking
//! of notify, sqlite, or axum — that's the whole point of layer 3a per ADR 0004.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Result;
use pdo_daemon::{serve_with_config, DaemonConfig, DaemonHandle};
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
    ///
    /// The daemon is seeded with a **harmless tmux command override** (a long
    /// `sleep`) so any node session it spawns runs that instead of launching a
    /// real `claude` process. This is per-daemon config — no process-global
    /// `std::env::set_var` — so parallel tests can't race on it (#181). Tests
    /// that need a different tail (e.g. an immediately-exiting command) use
    /// [`TestDaemon::spawn_with_override`].
    pub async fn spawn<F>(setup: F) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        Self::spawn_with_override(setup, Some("exec sleep 600".to_string())).await
    }

    /// Like [`TestDaemon::spawn`] but with an explicit tmux command override.
    ///
    /// - `Some(cmd)` → spawned node/manager sessions run `cmd` instead of claude.
    /// - `None` → real `claude` (no test should pass this; it exists only for
    ///   completeness / parity with production config).
    pub async fn spawn_with_override<F>(setup: F, tmux_cmd_override: Option<String>) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        // When the test suite itself runs inside a PDO node (e.g. an agent
        // worktree), `PDO_NODE_ID` is exported in the environment and the
        // daemon under test would consider itself "nested" — silently disabling
        // the orphan sweep and reaper, and failing every test that asserts on
        // them. A TestDaemon must behave like a top-level daemon regardless of
        // where the tests run; nested-mode tests opt back in explicitly via
        // `PDO_DAEMON_NO_CLEANUP=1`.
        std::env::remove_var("PDO_NODE_ID");

        let tempdir = tempfile::tempdir()?;
        setup(tempdir.path())?;

        let handle = serve_with_config(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            tempdir.path().to_path_buf(),
            DaemonConfig {
                tmux_cmd_override,
                panic_on_trigger_name: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon that **panics** the scheduler tick when a due Trigger named
    /// `panic_name` is processed (#222 fault injection). Lets a test prove the
    /// panic is isolated and the scheduler keeps firing. Per-daemon config, so no
    /// process-global env race (#181). Uses the same harmless `sleep` tmux tail as
    /// [`TestDaemon::spawn`].
    pub async fn spawn_with_panic_trigger<F>(setup: F, panic_name: &str) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        std::env::remove_var("PDO_NODE_ID");

        let tempdir = tempfile::tempdir()?;
        setup(tempdir.path())?;

        let handle = serve_with_config(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            tempdir.path().to_path_buf(),
            DaemonConfig {
                tmux_cmd_override: Some("exec sleep 600".to_string()),
                panic_on_trigger_name: Some(panic_name.to_string()),
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
            },
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
        pdo_daemon::tmux_session_manager::tmux_socket_name(self.addr.port())
    }

    /// Drive a single Trigger scheduler tick synchronously (test seam).
    pub async fn run_trigger_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_trigger_tick().await;
        }
    }

    /// Drive a single stale-detection sweep synchronously (test seam, #213).
    pub async fn run_stale_detection_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_stale_detection_tick().await;
        }
    }

    /// Run the boot-recovery reconciliation pass synchronously (test seam, #213).
    pub async fn run_boot_recovery_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_boot_recovery_tick().await;
        }
    }

    /// Arm the one-shot stale-sweep poison so the next stale-detection sweep
    /// panics, then disarms itself (#251 fault injection, test seam). Arm *after*
    /// boot so the immediate startup sweep doesn't consume it.
    pub fn arm_stale_panic(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.arm_stale_panic();
        }
    }

    /// Arm the one-shot spawn poison so the next `spawn_node` panics inside its
    /// post-worktree span, then disarms itself (#279 fault injection, test seam).
    /// Arm *after* boot and *before* the spawn under test (e.g. before `POST
    /// /runs` so the entry-node spawn consumes it).
    pub fn arm_spawn_panic(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.arm_spawn_panic();
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

        // Tear down the daemon's private tmux socket (#181). `task.abort()` only
        // stops the in-process axum task — the tmux *server* and the
        // `claude`/`sleep` children the daemon spawned via `tmux new-session`
        // are separate processes that would otherwise outlive the test, leaking
        // sessions and (without the command override) real claude. The socket is
        // scoped per daemon-port (`pdo-<port>`), so killing its server can
        // only reap *this* daemon's sessions — never another test's or a live
        // daemon's. Best-effort throughout: a missing socket / absent tmux is
        // fine.
        let socket = self.tmux_socket();
        let _ = std::process::Command::new("tmux")
            .args(["-L", &socket, "kill-server"])
            .output();

        // `kill-server` terminates the server but leaves the stale socket *file*
        // behind, and a test body that already killed the server itself leaves
        // one too. Unlink it so no `pdo-<port>` socket survives the test.
        // The socket name embeds this daemon's unique ephemeral port, so it is
        // ours alone — never the live daemon (`pdo-6172`) or a sibling test.
        // tmux stores sockets under `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/`; we don't
        // know our uid without libc, so unlink the file from every readable
        // `tmux-*` dir there (only the matching name is touched).
        let tmux_tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        if let Ok(entries) = std::fs::read_dir(&tmux_tmp) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("tmux-") {
                    let _ = std::fs::remove_file(entry.path().join(&socket));
                }
            }
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
