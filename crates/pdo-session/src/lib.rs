//! The `session` bounded context (ADR-0079): the tmux sessions PDO runs agents,
//! shells and the library assistant in, and everything a human does with them.
//!
//! - **Lifecycle**: session names, the script a session runs, its launch, the
//!   orphan sweep (reaper).
//! - **Text**: the daemon's messages to a harness agent (`[pdo-runtime]`).
//! - **Terminal**: the PTY bridge, the shared terminal (pilot and spectators),
//!   the OS terminal pop-out, a node's pane.
//! - **Declared wait**: `pdo wait-user`, lifted by the user's Enter.
//! - **Library assistant**: one shared authoring session, told which template is
//!   open through the focus.
//!
//! Hexagonal inside: `domain/` holds the pure rules, `application/` the ports
//! (tmux behind the one [`Tmux`](application::ports::Tmux) port) and one use case
//! per file, `adapters/` the routes and the implementations of the ports. The
//! composition root builds a [`Session`] from the adapters and serves
//! [`router`]. This crate depends on the kernel and the harness context only.

pub mod adapters;
pub mod application;
pub mod domain;

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;

use application::assistant::{
    AssistantLaunch, CloseAssistant, DeclareFocus, FocusCell, OpenAssistant, SaveFocused,
};
use application::attach::AttachTerminal;
use application::declared_wait::{DeclareWait, LiftWait, TypedInput};
use application::open_shell::OpenShell;
use application::ports::{
    AssistantHarness, DaemonGauges, NodePanes, NodeWaits, RunShells, SessionOwners, Templates,
    TerminalWindow, Tmux,
};
use application::read_pane::ReadPane;
use application::status::ReadStatus;
use application::sweep::{ReaperGauge, SweepOrphans};
use application::terminal::Terminals;

pub use adapters::http::router;

/// Every port the context needs, as the composition root plugs them in.
pub struct SessionPorts {
    pub tmux: Arc<dyn Tmux>,
    /// The Run facts behind a declared wait (implemented by the `run` side).
    pub waits: Arc<dyn NodeWaits>,
    /// The Run facts behind a node's pane (implemented by the `run` side).
    pub panes: Arc<dyn NodePanes>,
    /// The Runs a shell opens in (implemented by the `run` side).
    pub shells: Arc<dyn RunShells>,
    /// The owners of the live sessions (implemented by the `run` side).
    pub owners: Arc<dyn SessionOwners>,
    /// The status bar's daemon-wide gauges.
    pub gauges: Arc<dyn DaemonGauges>,
    /// The templates the assistant works on (implemented by the `pipeline` side).
    pub templates: Arc<dyn Templates>,
    pub assistant_harness: Arc<dyn AssistantHarness>,
    pub terminal_window: Arc<dyn TerminalWindow>,
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The daemon's knobs the context reads.
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub daemon_port: u16,
    pub repo_root: PathBuf,
    /// Replaces the harness tail of a session (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
    /// Extra WebSocket origins (`PDO_ALLOWED_WS_ORIGINS`), parsed at boot.
    pub allowed_ws_origins: Vec<String>,
}

/// The context, assembled: its use cases over its ports.
pub struct Session {
    tmux: Arc<dyn Tmux>,
    focus: Arc<FocusCell>,
    options: SessionOptions,
    declare_wait: DeclareWait,
    terminals: Terminals,
    sweep: SweepOrphans,
    read_status: ReadStatus,
    read_pane: ReadPane,
    open_shell: OpenShell,
    attach: AttachTerminal,
    open_assistant: OpenAssistant,
    close_assistant: CloseAssistant,
    declare_focus: DeclareFocus,
    save_focused: SaveFocused,
}

impl Session {
    pub fn new(ports: SessionPorts, options: SessionOptions) -> Arc<Self> {
        let SessionPorts {
            tmux,
            waits,
            panes,
            shells,
            owners,
            gauges,
            templates,
            assistant_harness,
            terminal_window,
            extensions,
        } = ports;
        let typed = Arc::new(TypedInput::default());
        let focus = Arc::new(FocusCell::default());
        let reaper = Arc::new(ReaperGauge::default());
        let launch = AssistantLaunch {
            repo_root: options.repo_root.clone(),
            daemon_port: options.daemon_port,
            tmux_cmd_override: options.tmux_cmd_override.clone(),
        };
        Arc::new(Self {
            declare_wait: DeclareWait::new(
                waits.clone(),
                tmux.clone(),
                typed.clone(),
                extensions.clone(),
            ),
            terminals: Terminals::new(
                tmux.clone(),
                LiftWait::new(waits, typed, extensions.clone()),
                extensions.clone(),
            ),
            sweep: SweepOrphans::new(tmux.clone(), owners, focus.clone(), reaper.clone()),
            read_status: ReadStatus::new(gauges, reaper),
            read_pane: ReadPane::new(panes, tmux.clone(), extensions.clone()),
            open_shell: OpenShell::new(
                shells,
                tmux.clone(),
                extensions.clone(),
                options.daemon_port,
            ),
            attach: AttachTerminal::new(tmux.clone(), terminal_window, extensions.clone()),
            open_assistant: OpenAssistant::new(
                tmux.clone(),
                assistant_harness,
                extensions.clone(),
                launch,
            ),
            close_assistant: CloseAssistant::new(tmux.clone(), focus.clone(), extensions.clone()),
            declare_focus: DeclareFocus::new(focus.clone(), templates.clone(), extensions.clone()),
            save_focused: SaveFocused::new(focus.clone(), templates, extensions),
            tmux,
            focus,
            options,
        })
    }

    /// tmux, on the daemon's private socket: the one way the rest of PDO
    /// reaches it.
    pub fn tmux(&self) -> &Arc<dyn Tmux> {
        &self.tmux
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    /// The declared wait (`pdo wait-user`, and the daemon's own on a refused
    /// completion).
    pub fn declare_wait(&self) -> &DeclareWait {
        &self.declare_wait
    }

    /// The orphan sweep, at boot and on the reaper interval.
    pub fn sweep(&self) -> &SweepOrphans {
        &self.sweep
    }

    /// The library assistant's focus.
    pub fn focus(&self) -> &FocusCell {
        &self.focus
    }

    /// The terminals the PTY bridge pilots.
    pub fn terminals(&self) -> &Terminals {
        &self.terminals
    }

    pub fn read_status(&self) -> &ReadStatus {
        &self.read_status
    }

    pub fn read_pane(&self) -> &ReadPane {
        &self.read_pane
    }

    pub fn open_shell(&self) -> &OpenShell {
        &self.open_shell
    }

    pub fn attach(&self) -> &AttachTerminal {
        &self.attach
    }

    pub fn open_assistant(&self) -> &OpenAssistant {
        &self.open_assistant
    }

    pub fn close_assistant(&self) -> &CloseAssistant {
        &self.close_assistant
    }

    pub fn declare_focus(&self) -> &DeclareFocus {
        &self.declare_focus
    }

    pub fn save_focused(&self) -> &SaveFocused {
        &self.save_focused
    }
}
