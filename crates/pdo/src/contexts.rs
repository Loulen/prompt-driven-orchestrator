//! The wiring of the bounded contexts (ADR-0079): each context's adapters
//! plugged onto its ports, over the resources the legacy daemon opened.

use std::sync::Arc;

use pdo_daemon::{ContextResources, Contexts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_session::adapters::terminal_window::OsTerminalWindow;
use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::{Session, SessionOptions, SessionPorts};

/// Wire every extracted context. Called once per daemon, at boot.
pub(crate) fn wire(resources: ContextResources, host: Arc<dyn ExtensionHost>) -> Contexts {
    let session = session(resources, host);
    Contexts {
        router: pdo_session::router(session.clone()),
        session,
    }
}

/// The `session` context: tmux on the daemon's private socket, the OS terminal
/// window, and the Run-side ports the legacy daemon still implements.
fn session(r: ContextResources, host: Arc<dyn ExtensionHost>) -> Arc<Session> {
    let run = r.session;
    Session::new(
        SessionPorts {
            tmux: Arc::new(TmuxCli::for_daemon(r.port)),
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
            repo_root: r.repo_root,
            tmux_cmd_override: r.tmux_cmd_override,
            allowed_ws_origins: r.allowed_ws_origins,
        },
    )
}
