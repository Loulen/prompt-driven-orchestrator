//! What the session use cases need from the outside world.
//!
//! [`Tmux`] is the single path through which PDO touches tmux: the legacy
//! crate, the PTY bridge and the shared terminal all go through it. The other
//! ports are the Run facts the session context reads without owning them —
//! implemented by the `run` side, which depends on this crate, never the
//! reverse.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use pdo_harness::registry::HarnessDescriptor;
use pdo_kernel::Actor;

use crate::domain::declared_wait::WaitFacts;
use crate::domain::launch::ContainerExec;
use crate::domain::pane::{PaneTarget, ReattachOutcome};
use crate::domain::reaper::{NodeRunInfo, SweepTtls};

/// A future a port returns: boxed, so the ports stay object-safe behind an
/// `Arc<dyn …>` (ADR-0079, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// tmux, bound to the daemon's private socket (`tmux -L pdo-<port>`).
///
/// Implemented by `adapters::tmux::TmuxCli`; a use case test injects an
/// in-memory one. Every call is blocking (it runs `tmux`).
pub trait Tmux: Send + Sync {
    /// The socket every call is pinned to.
    fn socket(&self) -> &str;

    /// The `PATH` a new session runs with: the user's interactive `PATH`, the one
    /// the harness preflight resolved the binary on (#661, ADR-0055).
    fn session_path(&self) -> String;

    /// `new-session -d -s <name> -c <working_dir> <script>`, then mouse mode on.
    /// `what` names the session kind in the error (`shell`, `resume`…).
    fn new_session(
        &self,
        name: &str,
        working_dir: &Path,
        script: &str,
        what: Option<&str>,
    ) -> anyhow::Result<()>;

    /// Kill a session. Best-effort: an absent session is not an error.
    fn kill(&self, name: &str);

    /// Whether the session exists.
    fn session_exists(&self, name: &str) -> bool;

    /// Whether the socket's server is alive at all (#234): `None` when the probe
    /// itself could not run, so absence is never read as a real "server gone".
    fn server_alive(&self) -> Option<bool>;

    /// Every `pdo-*` session on the socket.
    fn list_pdo_sessions(&self) -> HashSet<String>;

    /// Whether at least one client is attached to the session (#594).
    fn is_attached(&self, name: &str) -> bool;

    /// The visible pane (ANSI escapes kept), `None` when the session is gone.
    fn capture(&self, name: &str) -> Option<String>;

    /// Type `text` then Enter, as keystrokes. Best-effort.
    fn send_keys(&self, name: &str, text: &str);

    /// Hand `text` over as one bracketed paste, then Enter. Best-effort.
    fn paste(&self, name: &str, text: &str);

    /// Hand `text` over as one bracketed paste, **without** Enter: it waits in
    /// the session's input. Each call uses a buffer of its own, so concurrent
    /// pastes never mix. `false` when tmux refused the buffer or the paste.
    fn paste_without_enter(&self, name: &str, text: &str) -> bool;

    /// The argv that attaches a client to the session from a terminal: the PTY
    /// bridge runs it, `ignore_size` for a spectator (#867, ADR-0075).
    fn attach_argv(&self, name: &str, ignore_size: bool) -> Vec<String>;

    /// The same attach as one shell command line, for an OS terminal window.
    /// Always on the daemon's private socket, or the pop-out reaches another
    /// daemon's tmux server.
    fn attach_command(&self, name: &str) -> String;

    /// Flip `ignore-size` on the client run by `client_pid`. `false` when that
    /// client is not listed (yet) or tmux refused.
    fn set_client_ignore_size(&self, client_pid: u32, ignore_size: bool) -> bool;
}

/// An OS terminal window attached to a session (`POST /sessions/{id}/attach`).
pub trait TerminalWindow: Send + Sync {
    /// Open one running `attach` (a shell command line), and name the terminal
    /// program that was used.
    fn open(&self, session: &str, attach: &str) -> anyhow::Result<String>;
}

/// Why a Run could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunLookup {
    /// No such Run.
    NotFound,
    /// Its log could not be read.
    Storage(String),
}

/// The Run facts behind a declared wait (#588, ADR-0069). Implemented by the
/// `run` side.
pub trait NodeWaits: Send + Sync {
    /// What the Run's projection says about `(run, node)`; `Err` when the log
    /// could not be read.
    fn wait_facts<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<WaitFacts>, String>>;

    /// Append `NodeAwaitingUser` with `payload` (`{cause, message?}`).
    fn record_awaiting<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Append `NodeResumed` with `payload` (`{source}`).
    fn record_resumed<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// The Run facts behind a node's pane, and the in-place re-attach of its
/// session (ADR-0049 §3a). Implemented by the `run` side.
pub trait NodePanes: Send + Sync {
    /// Where `(run, node, iter)` stands, or `Ok(None)` when the node is not in
    /// the Run.
    fn pane_target<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, Result<Option<PaneTarget>, RunLookup>>;

    /// Resume the node's agent session in place.
    fn reattach<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        session: &'a str,
    ) -> BoxFuture<'a, ReattachOutcome>;
}

/// A terminal Run a shell may be opened in (ADR-0021).
pub struct ShellTarget {
    /// Terminal, not archived, worktree still on disk.
    pub eligible: bool,
    /// The Run's pipeline worktree.
    pub worktree: PathBuf,
    /// The `docker exec` of the Run's shell session into its container, `None`
    /// when the Run is not sandboxed.
    pub sandbox: Option<Box<dyn ContainerExec>>,
}

/// The Runs a shell opens in. Implemented by the `run` side.
pub trait RunShells: Send + Sync {
    fn shell_target<'a>(&'a self, run_id: &'a str)
        -> BoxFuture<'a, Result<ShellTarget, RunLookup>>;

    /// Make sure the Run's container is up: a host reboot leaves it stopped, and
    /// a sandboxed Run's shell must never fall back to a host bash.
    fn ensure_sandbox_ready<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// The owners of the live sessions, as the orphan sweep reads them (ADR-0038).
/// Implemented by the `run` side.
pub trait SessionOwners: Send + Sync {
    /// Each `(run_id, node_id)` owner's facts, in order; `None` for an owner
    /// absent from the log. `Err` aborts the sweep before it kills anything.
    fn owners<'a>(
        &'a self,
        owners: &'a [(String, String)],
    ) -> BoxFuture<'a, Result<Vec<Option<NodeRunInfo>>, String>>;

    /// The sweep's TTLs, `stored → env → default`, read fresh at every pass.
    fn ttls(&self) -> BoxFuture<'_, SweepTtls>;
}

/// The daemon-wide gauges of the status bar (`GET /sessions`).
pub trait DaemonGauges: Send + Sync {
    /// Live node sessions, daemon-wide.
    fn live_sessions(&self) -> BoxFuture<'_, usize>;
    /// The configured session cap.
    fn session_cap(&self) -> BoxFuture<'_, usize>;
    /// The persistent-service health, observed at boot.
    fn service_health(&self) -> serde_json::Value;
}

/// Why the assistant could not read or save the focused template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSaveError {
    /// The YAML does not parse: the message, and its line when known.
    Invalid {
        error: String,
        message: String,
        line: Option<usize>,
    },
    /// The template has no file yet: it has no draft either.
    NotFound,
    /// The draft could not be read or changed.
    Write(String),
}

/// The draft of a template, as the library assistant reads it: its YAML and
/// the prompts of its nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDraft {
    pub yaml: String,
    pub prompts: std::collections::BTreeMap<String, String>,
}

/// Where the assistant's save landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSaved {
    /// What changed in the draft.
    pub fields: Vec<String>,
    /// A file that does not read has no draft: the save was this version,
    /// as the editor's is on such a file.
    pub version: Option<u32>,
}

/// The pipeline templates the library assistant works on (ADR-0051), through
/// their shared drafts (ADR-0088): it never writes a file. Implemented by the
/// `pipeline` side.
pub trait Templates: Send + Sync {
    /// The file of template `pipeline_id` (it may not exist yet).
    fn path_of(&self, pipeline_id: &str) -> PathBuf;

    /// The draft of `pipeline_id`, as `actor`'s assistant reads it.
    fn read<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<TemplateDraft, TemplateSaveError>>;

    /// Save the whole template (YAML and prompts) into its draft, as `actor`'s
    /// assistant, live for every participant.
    fn save<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_id: &'a str,
        yaml: &'a str,
        prompts: &'a std::collections::HashMap<String, String>,
    ) -> BoxFuture<'a, Result<TemplateSaved, TemplateSaveError>>;

    /// `actor`'s assistant left `pipeline_id` (its focus moved, it closed):
    /// it leaves the draft of it, if it sat there.
    fn left(&self, actor: &Actor, pipeline_id: &str);
}

/// What the library assistant launches on: the instance's default harness,
/// resolved against the descriptor file (infra harness, no Run to follow).
pub trait AssistantHarness: Send + Sync {
    fn resolve(&self) -> BoxFuture<'_, HarnessDescriptor>;
}

/// The Projets of a Run (CORE-3): an Operation on a Run's node, session or
/// shell is in them. Implemented by the `run` side.
pub trait RunProjects: Send + Sync {
    /// The Projets `run_id` is in; none for an unknown Run.
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>>;

    /// The Projets of the Session hébergée `session`, from its record (never
    /// from its name); none by default and for a session not recorded.
    fn projects_of_hosted(&self, _session: &str) -> Vec<String> {
        Vec::new()
    }
}

/// No Run is in any Projet: a use case's Projets until it is given its own.
pub struct NoRunProjects;

impl RunProjects for NoRunProjects {
    fn projects_of_run<'a>(&'a self, _run_id: &'a str) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async { Vec::new() })
    }
}

/// The Projets of the session `session`: those of the Run it belongs to (a
/// node's, a manager's, a run shell's), the one a Session hébergée is
/// recorded in; none for the library assistant or a name PDO does not give.
pub async fn projects_of_session(projects: &dyn RunProjects, session: &str) -> Vec<String> {
    use crate::domain::names::{parse_session_name, ParsedSession};
    match parse_session_name(session) {
        Some(ParsedSession::Hosted) => projects.projects_of_hosted(session),
        Some(parsed) => match parsed.run_id() {
            Some(run_id) => projects.projects_of_run(run_id).await,
            None => Vec::new(),
        },
        None => Vec::new(),
    }
}
