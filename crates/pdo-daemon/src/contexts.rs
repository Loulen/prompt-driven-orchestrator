//! The seam between the legacy daemon and the bounded contexts already
//! extracted from it (ADR-0079).
//!
//! The legacy daemon opens the database and binds the listener; the composition
//! root wires each extracted context over these [`ContextResources`] and hands
//! back the [`Contexts`]. The ports a context needs from what is still legacy
//! (the Runs, the templates, the instance settings) are implemented in
//! `session_ports`, and move to their own crates when those contexts are
//! extracted.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use axum::Router;
use pdo_session::application::ports::{
    AssistantHarness, DaemonGauges, NodePanes, NodeWaits, RunShells, SessionOwners, Templates,
};
use pdo_session::Session;

use crate::session_ports::LegacySessionPorts;
use crate::AppState;

/// What the legacy daemon offers the contexts it hosts.
pub struct ContextResources {
    /// The daemon's repo root (its cwd).
    pub repo_root: PathBuf,
    /// The port the daemon listens on.
    pub port: u16,
    /// Replaces the harness tail of a session (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
    /// Extra WebSocket origins, parsed at boot.
    pub allowed_ws_origins: Vec<String>,
    /// The session context's Run-side ports.
    pub session: SessionRunPorts,
}

/// The ports of the session context that the Runs, the templates and the
/// instance settings implement.
pub struct SessionRunPorts {
    pub waits: Arc<dyn NodeWaits>,
    pub panes: Arc<dyn NodePanes>,
    pub shells: Arc<dyn RunShells>,
    pub owners: Arc<dyn SessionOwners>,
    pub gauges: Arc<dyn DaemonGauges>,
    pub templates: Arc<dyn Templates>,
    pub assistant_harness: Arc<dyn AssistantHarness>,
}

impl SessionRunPorts {
    /// Every port over the daemon's state, reached through `state`.
    pub(crate) fn over(state: Weak<AppState>) -> Self {
        let ports = Arc::new(LegacySessionPorts { state });
        Self {
            waits: ports.clone(),
            panes: ports.clone(),
            shells: ports.clone(),
            owners: ports.clone(),
            gauges: ports.clone(),
            templates: ports.clone(),
            assistant_harness: ports,
        }
    }
}

/// The extracted contexts, wired: each one's handle, and the routes they serve.
pub struct Contexts {
    pub session: Arc<Session>,
    pub router: Router,
}

/// The resources known before the state exists; the ports that reach back into
/// the state are added once it is being built.
pub(crate) struct ContextResourcesSeed {
    pub repo_root: PathBuf,
    pub port: u16,
    pub tmux_cmd_override: Option<String>,
    pub allowed_ws_origins: Vec<String>,
}

impl ContextResourcesSeed {
    pub(crate) fn into_resources(self, state: Weak<AppState>) -> ContextResources {
        ContextResources {
            repo_root: self.repo_root,
            port: self.port,
            tmux_cmd_override: self.tmux_cmd_override,
            allowed_ws_origins: self.allowed_ws_origins,
            session: SessionRunPorts::over(state),
        }
    }
}

/// The session context of a unit-test state: the real tmux adapter on the test
/// daemon's socket (no server runs there), the OS terminal window, and no
/// extension — or the `tmux` a test injects.
#[cfg(test)]
pub(crate) fn test_session(
    state: Weak<AppState>,
    repo_root: PathBuf,
    port: u16,
    tmux: Option<Arc<dyn pdo_session::application::ports::Tmux>>,
) -> Arc<Session> {
    let run = SessionRunPorts::over(state);
    Session::new(
        pdo_session::SessionPorts {
            tmux: tmux.unwrap_or_else(|| {
                Arc::new(pdo_session::adapters::tmux::TmuxCli::for_daemon(port))
            }),
            waits: run.waits,
            panes: run.panes,
            shells: run.shells,
            owners: run.owners,
            gauges: run.gauges,
            templates: run.templates,
            assistant_harness: run.assistant_harness,
            terminal_window: Arc::new(pdo_session::adapters::terminal_window::OsTerminalWindow),
            extensions: Arc::new(pdo_kernel::extension_host::NoExtensions),
        },
        pdo_session::SessionOptions {
            daemon_port: port,
            repo_root,
            tmux_cmd_override: Some("exec true".to_string()),
            allowed_ws_origins: Vec::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use pdo_session::domain::declared_wait::{CAUSE_COMPLETION_NOT_RELEASED, CAUSE_DECLARED};

    use crate::event_log::{AWAITING_CAUSE_COMPLETION_NOT_RELEASED, AWAITING_CAUSE_DECLARED};

    /// The session context declares a wait with its own causes; the Run's
    /// projection folds them under the same names.
    #[test]
    fn the_wait_causes_are_the_ones_the_projection_folds() {
        assert_eq!(CAUSE_DECLARED, AWAITING_CAUSE_DECLARED);
        assert_eq!(
            CAUSE_COMPLETION_NOT_RELEASED,
            AWAITING_CAUSE_COMPLETION_NOT_RELEASED
        );
    }
}
