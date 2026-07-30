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
                docker_cmd_override: None,
                sandbox_home_override: None,
                price_source_url: None,
                price_refresh_at_boot: false,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon whose sandbox wiring shells out to a **fake `docker`** (#407).
    ///
    /// `docker_cmd` is the command every sandbox docker call runs instead of the
    /// real binary (e.g. a script that logs its argv and canned-responds to
    /// `inspect`/`create`/`start`/`exec`/`rm`). The tmux tail stays the harmless
    /// `exec true` so a sandboxed node's *host-side* wrapper collapses instantly —
    /// no real claude, no lingering session. Per-daemon config, no `std::env`
    /// race (#181). No docker teardown in `Drop`: the fake creates no real
    /// container.
    pub async fn spawn_with_docker_override<F>(setup: F, docker_cmd: String) -> Result<Self>
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
                tmux_cmd_override: Some("exec true".to_string()),
                panic_on_trigger_name: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
                docker_cmd_override: Some(docker_cmd),
                // #407: stage the sandbox home UNDER the tempdir so the test never
                // touches the real `$HOME` (`~/.pdo/sandbox`, `~/.claude`).
                sandbox_home_override: Some(tempdir.path().to_path_buf()),
                price_source_url: None,
                price_refresh_at_boot: false,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon whose `$HOME` for transcript reads is the tempdir (#469).
    ///
    /// `sandbox_home_override` is the seam `sandbox_run::transcripts_root` resolves
    /// against, and for an `off` Run it *is* the host home — so a test can plant a
    /// Claude Code transcript at `<repo_root>/.claude/projects/<encoded cwd>/` and
    /// the sweep will read it, without touching the real `~/.claude` and without a
    /// process-global `std::env::set_var("HOME", …)` (which would confine the whole
    /// binary to a single test, #181).
    ///
    /// No fake docker: these tests exercise the `off` path.
    pub async fn spawn_with_home_override<F>(
        setup: F,
        tmux_cmd_override: Option<String>,
    ) -> Result<Self>
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
                tmux_cmd_override,
                panic_on_trigger_name: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
                docker_cmd_override: None,
                sandbox_home_override: Some(tempdir.path().to_path_buf()),
                price_source_url: None,
                price_refresh_at_boot: false,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon with a **fake `docker`** AND an explicit tmux tail override
    /// (#408). Like [`TestDaemon::spawn_with_docker_override`] but the caller
    /// chooses the tail: pass `Some("exec sleep 600")` so a sandboxed node's
    /// session stays **alive** long enough to exercise the live-run observability
    /// paths (cost + stale-detection reading the staged home). The default
    /// docker-override harness pins the tail to `exec true`, which collapses the
    /// session instantly — too short to prove "stale reads the staging".
    ///
    /// `sandbox_home_override` is the tempdir, so the staging
    /// (`<tempdir>/.pdo/sandbox/<run>/…`) and the host `~/.claude`
    /// (`<tempdir>/.claude/…`) both live under the test's own dir — hermetic, no
    /// real `$HOME` touched.
    pub async fn spawn_with_docker_and_tmux_override<F>(
        setup: F,
        docker_cmd: String,
        tmux_cmd_override: Option<String>,
    ) -> Result<Self>
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
                tmux_cmd_override,
                panic_on_trigger_name: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
                docker_cmd_override: Some(docker_cmd),
                sandbox_home_override: Some(tempdir.path().to_path_buf()),
                price_source_url: None,
                price_refresh_at_boot: false,
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
                docker_cmd_override: None,
                sandbox_home_override: None,
                price_source_url: None,
                price_refresh_at_boot: false,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon whose price source is `price_source_url` and whose boot
    /// refresh is ARMED (#427).
    ///
    /// `sandbox_home_override` is the tempdir, so `~/.pdo/prices/` resolves under
    /// the test's own dir — the price files are read from the HOST home root, so
    /// without this the test would read (and the sync would WRITE) the real
    /// `~/.pdo/prices/`.
    ///
    /// `price_refresh_at_boot: true` is deliberate and safe: the boot pass only
    /// refreshes an EXISTING `fetched.json`, and a fresh tempdir has none — so a
    /// test that plants no cache proves "zero request" against production's own
    /// gate, and one that plants a stale cache drives the refresh through
    /// [`pdo_daemon::DaemonHandle::run_price_refresh_tick`] rather than racing the
    /// detached boot task.
    pub async fn spawn_with_price_source<F>(setup: F, price_source_url: String) -> Result<Self>
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
                tmux_cmd_override: Some("exec true".to_string()),
                panic_on_trigger_name: None,
                panic_on_stale_sweep: false,
                panic_on_spawn: false,
                service_health_override: None,
                docker_cmd_override: None,
                sandbox_home_override: Some(tempdir.path().to_path_buf()),
                price_source_url: Some(price_source_url),
                price_refresh_at_boot: true,
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

    /// The `target_repo` a test Run or Trigger should name (#470, ADR-0033).
    ///
    /// `target_repo` is **required** at every write boundary — the daemon's own
    /// working directory is no longer an implicit Run target. Every test in this
    /// directory was written against that old implicit default, so naming the
    /// daemon root here keeps the exact same semantics, now stated explicitly.
    /// Put this in every `POST /runs` and `POST /triggers` body.
    pub fn target_repo(&self) -> String {
        self.repo_root().to_string_lossy().into_owned()
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

    /// Run the boot price-table refresh synchronously (test seam, #427). Production
    /// spawns this DETACHED at startup; a test must drive it rather than race it.
    pub async fn run_price_refresh_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_price_refresh_tick().await;
        }
    }

    /// Run a single orphan-sweep (reaper) pass synchronously (test seam, #316).
    pub async fn run_orphan_sweep_tick(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.run_orphan_sweep_tick().await;
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

    /// Arm the terminal-tail gate (#304 fault injection, test seam): the
    /// detached tail of the next `node_done`/`node_fail`/`node_skip` parks at
    /// its head until [`Self::release_node_done_gate`], holding the run-advance
    /// window open so the test can drop the client connection inside it.
    pub fn arm_node_done_gate(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.arm_node_done_gate();
        }
    }

    /// Release the terminal-tail gate (#304): disarm and wake parked tails.
    pub fn release_node_done_gate(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.release_node_done_gate();
        }
    }

    /// Force a Trigger's next fire into the past so the next tick treats it as
    /// due (test seam).
    pub async fn force_trigger_due(&self, trigger_id: &str) {
        if let Some(handle) = self.handle.as_ref() {
            handle.force_trigger_due(trigger_id).await;
        }
    }

    /// Seed a Trigger row with a NULL `target_repo` — a **pre-#470 record**
    /// (ADR-0033). `POST /triggers` refuses that shape now, so a test that needs
    /// one has to go under the API. Returns the new trigger id.
    pub async fn seed_legacy_trigger_without_target_repo(
        &self,
        name: &str,
        pipeline_id: &str,
        cron: &str,
        guard_command: Option<&str>,
    ) -> String {
        self.handle
            .as_ref()
            .expect("daemon handle")
            .seed_legacy_trigger_without_target_repo(name, pipeline_id, cron, guard_command)
            .await
    }

    /// Seed a `run_started` event with no `target_repo` — a pre-#470 Run record
    /// (ADR-0033). Same reason as the Trigger seam above.
    pub async fn seed_legacy_run_without_target_repo(&self, run_id: &str, pipeline_name: &str) {
        self.handle
            .as_ref()
            .expect("daemon handle")
            .seed_legacy_run_without_target_repo(run_id, pipeline_name)
            .await;
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
