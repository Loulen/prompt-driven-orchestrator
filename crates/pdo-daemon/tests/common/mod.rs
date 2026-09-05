//! Shared test harness for Cargo integration tests (testing pyramid layer 3a).
//!
//! Boots a real daemon on an ephemeral port over a `tempfile::TempDir`. No mocking
//! of notify, sqlite, or axum — that's the whole point of layer 3a per ADR 0004.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

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
        Self::spawn_inner(setup, tmux_cmd_override, false).await
    }

    /// Spawn a daemon in **nested (no-cleanup) mode**: no boot orphan sweep, no
    /// periodic reaper, no boot recovery, no stale detector — completely passive on
    /// tmux state, exactly as when a sub-claude accidentally runs `pdo daemon`.
    ///
    /// Two kinds of test need it: one that asserts the passivity itself, and one
    /// that creates a tmux session **out of band** (no run in the event log), which
    /// an armed reaper kills on sight as an unrecognised `pdo-*` name — with no TTL
    /// grace on that arm.
    ///
    /// It is a `DaemonConfig` field and not `set_var("PDO_DAEMON_NO_CLEANUP")`
    /// because the env var is process-global while a test binary holds several
    /// daemons: the sibling `remove_var` used to land inside another test's
    /// in-flight `serve_with_config` and boot *that* daemon with cleanup armed.
    /// See `DaemonConfig::nested_daemon`.
    pub async fn spawn_nested<F>(setup: F) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        Self::spawn_inner(setup, Some("exec sleep 600".to_string()), true).await
    }

    async fn spawn_inner<F>(
        setup: F,
        tmux_cmd_override: Option<String>,
        nested_daemon: bool,
    ) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
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
                allowed_ws_origins: Vec::new(),
                // #450: tests drive firing via the `run_trigger_tick` seam; the
                // heartbeat's boot tick would race it.
                run_trigger_scheduler_loop: false,
                // A TestDaemon is a TOP-LEVEL daemon (sweeps armed) whatever env
                // the suite was launched with; `spawn_nested` is the only opt-out.
                nested_daemon,
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
                allowed_ws_origins: Vec::new(),
                // #450: tests drive firing via the `run_trigger_tick` seam; the
                // heartbeat's boot tick would race it.
                run_trigger_scheduler_loop: false,
                // See the sibling constructors: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
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
                allowed_ws_origins: Vec::new(),
                // #450: tests drive firing via the `run_trigger_tick` seam; the
                // heartbeat's boot tick would race it.
                run_trigger_scheduler_loop: false,
                // See the sibling constructors: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
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
                allowed_ws_origins: Vec::new(),
                // #450: tests drive firing via the `run_trigger_tick` seam; the
                // heartbeat's boot tick would race it.
                run_trigger_scheduler_loop: false,
                // See the sibling constructors: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
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
                allowed_ws_origins: Vec::new(),
                // #450: tests drive firing via the `run_trigger_tick` seam; the
                // heartbeat's boot tick would race it.
                run_trigger_scheduler_loop: false,
                // See the sibling constructors: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
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
                allowed_ws_origins: Vec::new(),
                // #450: deterministic tick seam — no background heartbeat.
                run_trigger_scheduler_loop: false,
                // See the sibling literals: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
            },
        )
        .await?;

        Ok(Self {
            addr: handle.addr,
            tempdir,
            handle: Some(handle),
        })
    }

    /// Spawn a daemon whose WebSocket Origin allowlist is EXTENDED with `origins`
    /// (#564), as if `PDO_ALLOWED_WS_ORIGINS` were set. Additive: the four
    /// localhost/127.0.0.1 defaults still pass, so a test can assert both that a
    /// configured public origin is accepted and that loopback keeps working.
    ///
    /// Per-daemon config, never a process-global `std::env::set_var` (#181): the
    /// env is read exactly once in `DaemonConfig::from_env`, and the seam a test
    /// drives is this `DaemonConfig` field. The daemon binds an ephemeral port,
    /// so an allowlist entry must be PORT-INDEPENDENT (a public domain, not
    /// `127.0.0.1:<port>` — the defaults already cover loopback).
    pub async fn spawn_with_allowed_ws_origins<F>(setup: F, origins: Vec<String>) -> Result<Self>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
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
                sandbox_home_override: None,
                price_source_url: None,
                price_refresh_at_boot: false,
                // #450: deterministic tick seam — no background heartbeat.
                run_trigger_scheduler_loop: false,
                // See the sibling literals: armed sweeps, per-daemon opt-out.
                nested_daemon: false,
                allowed_ws_origins: origins,
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

    /// The Claude Code session id this daemon pinned for `node_id`'s latest
    /// iteration, read back from its `node_started` payload (#473).
    ///
    /// Mandatory for any test that **plants** a transcript. Since #473 the liveness
    /// sweep resolves a node's transcript by exact filename — `<session_id>.jsonl`
    /// under the encoded-cwd project dir — instead of picking the newest `.jsonl`
    /// in that dir, so a fixture written under any other name resolves to nothing
    /// and the sweep sees "no signal" rather than the planted turn state. The id is
    /// a fresh uuid per spawn, so reading it back is the only way to know it.
    ///
    /// Panics if the node has no `node_started` yet, or none carrying an id (a
    /// `script` node legitimately has none — it launches no agent and nothing would
    /// resolve its transcript anyway).
    pub async fn pinned_session_id(&self, run_id: &str, node_id: &str) -> String {
        let events: Vec<serde_json::Value> = reqwest::Client::new()
            .get(format!("{}/runs/{run_id}/events", self.url()))
            .send()
            .await
            .expect("GET /runs/<id>/events")
            .json()
            .await
            .expect("events decode as JSON");

        // The LAST `node_started` for this node: a `restart_node` of the same
        // iteration pins a fresh id, and the sweep reads back the latest one.
        events
            .iter()
            .filter(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
            .filter_map(|e| e["payload"]["session_id"].as_str())
            .next_back()
            .unwrap_or_else(|| {
                panic!(
                    "no node_started with a pinned session_id for {node_id} in run {run_id} \
                     — plant the transcript AFTER the node has started"
                )
            })
            .to_string()
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

// Process-global env vars: shared serial guards.
//
// The whole `tests/` tree now compiles into ONE binary (`tests/it.rs`), so
// `cargo test` runs every test as a thread in a single process. Env vars that
// used to be safe because "this file is the only one that touches it, and each
// file is its own process" are no longer safe: two files setting the same var
// now overlap, and the first to finish unsets it under the other.
//
// The locks live here, not in a test file, so tests in *any* file contend on the
// same mutex.
//
// `EnvVarGuard` restores on `Drop`, so a panicking test still puts the previous
// value back and still releases the lock — a manual `remove_var` at the end of
// the test body does neither.

/// Serialises `PDO_SESSION_CAP` between `session_cap_admission.rs` and
/// `admission_concurrency.rs` — both set it, to different values, and both
/// assert on the cap they set.
static SESSION_CAP_LOCK: Mutex<()> = Mutex::new(());

/// Serialises `PDO_GUARD_TIMEOUT_MS` between `guard_dry_run_timeout.rs` and
/// `trigger_scheduler.rs`.
static GUARD_TIMEOUT_LOCK: Mutex<()> = Mutex::new(());

/// Holds a contended env var at a chosen value, and holds the lock that makes
/// that exclusive, for as long as the guard is alive.
///
/// `Drop` restores the previous value (or unsets it if there was none) *before*
/// releasing the lock — the `_lock` field is declared last, so it is dropped
/// last. Bind it to a named local (`let _cap = …`), never to `_`: `let _ = …`
/// drops it immediately and the protection evaporates.
pub struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
    // Dropped after the `Drop` impl below has restored `key`.
    _lock: MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    /// Take `lock`, then snapshot and overwrite `key`.
    ///
    /// The snapshot is taken *after* the lock is held. Reading it earlier is the
    /// bug this replaces: a test that snapshotted first could observe a value a
    /// concurrent test had just set, and then "restore" that foreign value
    /// permanently once the other test had already removed it.
    fn acquire(lock: &'static Mutex<()>, key: &'static str, value: &str) -> Self {
        // Poisoning is tolerated on purpose: one panicking test must not cascade
        // into false failures in every other test that wants this var.
        let _lock = lock.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key,
            previous,
            _lock,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Set `PDO_SESSION_CAP` for the lifetime of the returned guard, excluding any
/// other test that also wants it.
#[must_use = "the cap is restored as soon as the guard is dropped"]
pub fn lock_session_cap(value: impl AsRef<str>) -> EnvVarGuard {
    EnvVarGuard::acquire(
        &SESSION_CAP_LOCK,
        pdo_daemon::admission::SESSION_CAP_ENV,
        value.as_ref(),
    )
}

/// Set `PDO_GUARD_TIMEOUT_MS` for the lifetime of the returned guard, excluding
/// any other test that also wants it.
#[must_use = "the timeout override is restored as soon as the guard is dropped"]
pub fn lock_guard_timeout_ms(value: impl AsRef<str>) -> EnvVarGuard {
    EnvVarGuard::acquire(
        &GUARD_TIMEOUT_LOCK,
        pdo_daemon::GUARD_TIMEOUT_MS_OVERRIDE_ENV,
        value.as_ref(),
    )
}

/// The process-wide directory the catalogue tests install their **fake harness
/// binaries** in, with the harness probe `PATH` fixed to `<this dir>:<process PATH>`
/// — once per process, on first call.
///
/// Why one dir and why the process PATH appended: `PDO_HARNESS_PROBE_PATH` is read
/// by *every* session spawn in this test binary (the live session inherits the very
/// PATH the preflight resolved the harness on, ADR-0055), and `cargo test` runs all
/// the integration modules as threads of ONE process. A test that pointed the
/// variable at a bare tempdir of fakes — then dropped that tempdir — left every
/// later-spawned session (libassist, script nodes, shells…) with a PATH holding
/// neither `bash`, `tmux` nor `pdo`: they died at once, and dozens of unrelated
/// tests failed whenever the catalogue tests ran in the same process. The fakes
/// still win (their dir comes first), the sessions still boot (the real tools come
/// after), and the dir outlives every test.
///
/// Also collapses the catalogue version TTL to a millisecond, so the
/// re-probe-on-version-change contract is provable without waiting a minute.
pub fn fake_harness_bindir() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        ensure_pdo_on_path();
        let dir = std::env::temp_dir().join(format!("pdo-fake-harness-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fake harness bindir is creatable");
        let process_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PDO_HARNESS_PROBE_PATH",
            format!("{}:{process_path}", dir.display()),
        );
        std::env::set_var(pdo_daemon::CATALOGUE_VERSION_TTL_MS_ENV, "1");
        dir
    })
    .clone()
}

/// Prepend the directory holding the freshly built `pdo` binary to `PATH`, once
/// per process. Shared so per-file `Once`s can't read-modify-write `PATH`
/// concurrently.
pub fn ensure_pdo_on_path() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let bin = Path::new(env!("CARGO_BIN_EXE_pdo"));
        let dir = bin.parent().expect("pdo binary has a parent dir");
        let existing = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), existing));
    });
}
