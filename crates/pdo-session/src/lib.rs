//! The `session` bounded context (ADR-0079): the tmux sessions PDO runs agents,
//! shells and the library assistant in, and everything a human does with them.
//!
//! - **Lifecycle**: session names, the script a session runs, its launch, the
//!   orphan sweep (reaper).
//! - **Text**: the daemon's messages to a harness agent (`[pdo-runtime]`).
//! - **Terminal**: the PTY bridge, the shared terminal (pilot and spectators),
//!   the OS terminal pop-out, a node's pane.
//! - **Declared wait**: `pdo wait-user`, lifted by the user's Enter.
//! - **Terminal text**: a text written into a node's input on the user's
//!   gesture, without Enter.
//! - **Library assistant**: one authoring session per Actor, told which
//!   template its Actor has open through its focus.
//! - **Session ledger**: who started each session, who takes control of its
//!   terminal, when it ends, as the extensions are told (CORE-61).
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pdo_kernel::extension_host::ExtensionHost;

use application::assistant::{
    AssistantLaunch, CloseAssistant, DeclareFocus, FocusCell, OpenAssistant, SaveFocused,
};
use application::attach::AttachTerminal;
use application::declared_wait::{DeclareWait, LiftWait, TypedInput};
use application::open_shell::OpenShell;
use application::ports::{
    AssistantHarness, DaemonGauges, NodePanes, NodeWaits, RunProjects, RunShells, SessionOwners,
    Templates, TerminalWindow, Tmux,
};
use application::read_pane::ReadPane;
use application::session_ledger::SessionLedger;
use application::status::ReadStatus;
use application::sweep::{ReaperGauge, SweepOrphans};
use application::terminal::Terminals;
use application::terminal_text::SendTerminalText;

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
    /// The Projets of a Run, its sessions' Operations are in (CORE-3).
    pub projects: Arc<dyn RunProjects>,
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
    ledger: Arc<SessionLedger>,
    options: SessionOptions,
    declare_wait: DeclareWait,
    send_terminal_text: SendTerminalText,
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
    /// How often the PTY bridge beats a terminal socket, in ms: the
    /// [`TERMINAL_BEAT_INTERVAL`](domain::pty_protocol::TERMINAL_BEAT_INTERVAL),
    /// shortened by tests.
    terminal_beat_ms: AtomicU64,
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
            projects,
        } = ports;
        let typed = Arc::new(TypedInput::default());
        let focus = Arc::new(FocusCell::default());
        let reaper = Arc::new(ReaperGauge::default());
        let ledger = Arc::new(SessionLedger::new(tmux.clone(), extensions.clone()));
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
            )
            .with_projects(projects.clone()),
            send_terminal_text: SendTerminalText::new(
                waits.clone(),
                tmux.clone(),
                typed.clone(),
                extensions.clone(),
            )
            .with_projects(projects.clone()),
            terminals: Terminals::new(
                tmux.clone(),
                LiftWait::new(waits, typed, extensions.clone()),
                extensions.clone(),
            )
            .with_projects(projects.clone())
            .with_ledger(ledger.clone()),
            sweep: SweepOrphans::new(tmux.clone(), owners, focus.clone(), reaper.clone()),
            read_status: ReadStatus::new(gauges, reaper),
            read_pane: ReadPane::new(panes, tmux.clone(), extensions.clone())
                .with_projects(projects.clone()),
            open_shell: OpenShell::new(
                shells,
                tmux.clone(),
                extensions.clone(),
                options.daemon_port,
            )
            .with_projects(projects.clone())
            .with_ledger(ledger.clone()),
            attach: AttachTerminal::new(tmux.clone(), terminal_window, extensions.clone())
                .with_projects(projects),
            open_assistant: OpenAssistant::new(
                tmux.clone(),
                assistant_harness,
                extensions.clone(),
                launch,
            )
            .with_ledger(ledger.clone()),
            close_assistant: CloseAssistant::new(tmux.clone(), focus.clone(), extensions.clone()),
            declare_focus: DeclareFocus::new(focus.clone(), templates.clone(), extensions.clone()),
            save_focused: SaveFocused::new(focus.clone(), templates, extensions),
            tmux,
            focus,
            ledger,
            options,
            terminal_beat_ms: AtomicU64::new(
                domain::pty_protocol::TERMINAL_BEAT_INTERVAL.as_millis() as u64,
            ),
        })
    }

    /// How often the PTY bridge beats the terminal sockets it opens.
    pub fn terminal_beat_interval(&self) -> Duration {
        Duration::from_millis(self.terminal_beat_ms.load(Ordering::Relaxed).max(1))
    }

    /// Change the beat of the terminal sockets opened afterwards (test seam).
    pub fn set_terminal_beat_interval(&self, interval: Duration) {
        self.terminal_beat_ms
            .store(interval.as_millis() as u64, Ordering::Relaxed);
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

    /// « Copy & send to terminal »: a text into a node's input, no Enter.
    pub fn send_terminal_text(&self) -> &SendTerminalText {
        &self.send_terminal_text
    }

    /// The orphan sweep, at boot and on the reaper interval.
    pub fn sweep(&self) -> &SweepOrphans {
        &self.sweep
    }

    /// The focus of each library assistant.
    pub fn focus(&self) -> &FocusCell {
        &self.focus
    }

    /// The live sessions, as the extensions are told about them: who started
    /// each, who takes control, when it ends (CORE-61).
    pub fn ledger(&self) -> &Arc<SessionLedger> {
        &self.ledger
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
