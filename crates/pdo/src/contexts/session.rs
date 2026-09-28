//! The `session` context: tmux on the daemon's private socket (or the one a
//! test hands in), the OS terminal window, and its Run-side ports over the
//! Runs, the platform's service health and the pipeline registry.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::Platform;
use pdo_run::Runs;
use pdo_session::adapters::terminal_window::OsTerminalWindow;
use pdo_session::application::ports::Tmux;
use pdo_session::{Session, SessionOptions, SessionPorts};

use super::ContextResources;
use crate::ports::session::{RunSide, SessionRunPorts};
use crate::ports::Late;

pub(super) fn session(
    r: &ContextResources,
    tmux: Arc<dyn Tmux>,
    runs: &Late<Runs>,
    platform: &Late<Platform>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Session> {
    let run = SessionRunPorts::over(RunSide {
        runs: runs.clone(),
        platform: platform.clone(),
        db: r.db.clone(),
        repo_root: r.repo_root.clone(),
        self_writes: r.self_writes.clone(),
        bus_tx: r.bus_tx.clone(),
        docker_cmd_override: r.docker_cmd_override.clone(),
    });
    Session::new(
        SessionPorts {
            tmux,
            waits: run.waits,
            panes: run.panes,
            shells: run.shells,
            owners: run.owners,
            gauges: run.gauges,
            templates: run.templates,
            assistant_harness: run.assistant_harness,
            terminal_window: Arc::new(OsTerminalWindow),
            extensions: host,
        },
        SessionOptions {
            daemon_port: r.port,
            repo_root: r.repo_root.clone(),
            tmux_cmd_override: r.tmux_cmd_override.clone(),
            allowed_ws_origins: r.allowed_ws_origins.clone(),
        },
    )
}
