//! What the platform context needs from the outside, as traits. The adapters of
//! `adapters/` implement the technical ones (SQLite, files, processes, HTTP);
//! the contexts downstream implement the ones about them (the Runs, the
//! instance settings). The composition root plugs them in as `Arc<dyn Port>`.

use std::ffi::OsString;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::domain::attempt::UpdateAttempt;
use crate::domain::audit::{AuditEntry, AuditFilter, NewAuditEntry};
use crate::domain::install::{InstallMethod, Supervision};
use crate::domain::page_mount::{MountError, PageMount};
use crate::domain::release::Release;
use crate::domain::service_unit::PortState;
use crate::domain::update_check::UpdateCache;
use crate::domain::update_plan::UpdatePlan;

/// The future a port returns: boxed, so the port stays object-safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A storage failure, with the message the log shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StorageError(pub String);

/// Why a mount could not be stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InsertError {
    /// The name is taken.
    #[error("duplicate page mount")]
    Duplicate,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Where page mounts are kept.
pub trait PageMountRepository: Send + Sync {
    /// Create the storage if it does not exist yet.
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>>;
    /// Store a new mount; [`InsertError::Duplicate`] when its name is taken.
    fn insert<'a>(&'a self, mount: &'a PageMount) -> BoxFuture<'a, Result<(), InsertError>>;
    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>>;
    /// Every mount, by name.
    fn list(&self) -> BoxFuture<'_, Result<Vec<PageMount>, StorageError>>;
    /// Remove a mount, returning what was removed.
    fn remove<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>>;
    /// Remove every mount a Run made; how many went.
    fn remove_for_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<u64, StorageError>>;
}

/// A file read beneath a mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageFile {
    pub contents: Vec<u8>,
    /// The name the content type is guessed from (`index.html` for a directory).
    pub mime_path: PathBuf,
}

/// The files behind the mounts.
pub trait PageFiles: Send + Sync {
    /// The canonical form of a directory to mount, refused when it is not one.
    fn canonical_directory(&self, directory: &Path) -> Result<PathBuf, MountError>;
    /// Read the file `components` names beneath the mount root, never leaving
    /// it and never following a symlink. `Ok(None)` when there is no such file.
    fn read<'a>(
        &'a self,
        root: &'a Path,
        components: &'a [OsString],
    ) -> BoxFuture<'a, Result<Option<PageFile>, StorageError>>;
    /// Whether [`Self::read`] would find a file there, without reading it.
    fn exists<'a>(&'a self, root: &'a Path, components: &'a [OsString]) -> BoxFuture<'a, bool>;
}

/// The Runs, as the platform sees them. Implemented by the `run` context.
pub trait Runs: Send + Sync {
    /// Accept the claim of an agent to speak from the live session of
    /// `node_id` in `run_id`, or refuse it with the reason.
    fn verify<'a>(&'a self, run_id: &'a str, node_id: &'a str)
        -> BoxFuture<'a, Result<(), String>>;
    /// Journal, in the Run's event log, a command the agent of that session
    /// issued about a page mount.
    fn record_command<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        command: &'a str,
        page_mount: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// How many Runs are live (running, awaiting the user, paused).
    fn count_live(&self) -> BoxFuture<'_, usize>;
    /// The ids of the nodes of `run_id`; `None` when there is no such Run.
    fn node_ids<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Option<Vec<String>>>;
    /// The Projets `run_id` was launched in; none for an unknown Run or a Run
    /// launched in no Projet.
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>>;
}

/// The out-of-Run audit journal.
pub trait AuditLog: Send + Sync {
    /// Create the storage if it does not exist yet.
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>>;
    /// Write one row, stamped now.
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, Result<(), StorageError>>;
    /// The rows the filter selects, newest first.
    fn list<'a>(
        &'a self,
        filter: &'a AuditFilter,
    ) -> BoxFuture<'a, Result<Vec<AuditEntry>, StorageError>>;
}

/// The `update_check` instance setting. Implemented by the `settings` context.
pub trait UpdateCheckSetting: Send + Sync {
    /// Whether the version check may run, resolved fresh (`stored → env →
    /// default`) so a toggle takes effect without a restart.
    fn enabled(&self) -> BoxFuture<'_, bool>;
}

/// The `public_url` instance setting. Implemented by the `settings` context.
pub trait PublicUrlSetting: Send + Sync {
    /// The address humans open PDO at, resolved fresh (`stored → env → none`)
    /// so a change takes effect without a restart.
    fn resolve(&self) -> BoxFuture<'_, Option<String>>;
}

/// The release source.
pub trait ReleaseSource: Send + Sync {
    /// The latest published version at `url`.
    fn latest<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<String, String>>;
    /// The published releases listed at `url`.
    fn releases<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<Release>, String>>;
}

/// The update journal: the check cache, the last attempt, the scripts and logs.
pub trait UpdateJournal: Send + Sync {
    /// `false` when there is nowhere to write it (no home directory).
    fn is_available(&self) -> bool;
    fn read_cache(&self) -> Option<UpdateCache>;
    fn write_cache(&self, cache: &UpdateCache) -> Result<(), String>;
    fn read_attempt(&self) -> Option<UpdateAttempt>;
    fn write_attempt(&self, attempt: &UpdateAttempt) -> Result<(), String>;
    /// Where the attempt record lives.
    fn attempt_path(&self) -> PathBuf;
    /// Where the log of an attempt lives.
    fn log_path(&self, attempt_id: &str) -> PathBuf;
    /// Write the script of an attempt; where it was written.
    fn write_script(&self, attempt_id: &str, script: &str) -> Result<PathBuf, String>;
    /// Append a line to an attempt's log, best-effort.
    fn append_log(&self, attempt_id: &str, line: &str);
    /// An attempt's log; `Ok(None)` when there is none.
    fn read_log(&self, attempt_id: &str) -> Result<Option<String>, String>;
}

/// An executor that was spawned and runs on its own.
pub trait RunningExecutor: Send {
    fn pid(&self) -> u32;
    /// Block until it exits; its exit code (`-1` when killed by a signal),
    /// `None` when it cannot be waited on.
    fn wait(self: Box<Self>) -> Option<i32>;
}

/// What runs an update plan, detached from the daemon.
pub trait UpdateExecutor: Send + Sync {
    fn spawn(&self, plan: &UpdatePlan, script: &Path) -> Result<Box<dyn RunningExecutor>, String>;
}

/// How this binary was installed and is supervised.
pub trait Installation: Send + Sync {
    fn method(&self) -> InstallMethod;
    fn supervision(&self) -> Supervision;
    /// The path the binary was invoked by (never the resolved Cellar target).
    fn invoked_exe(&self) -> Option<PathBuf>;
}

/// Outcome of a spawned side-effect command (`systemctl` / `loginctl` / `launchctl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdOutcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// The host `pdo service` installs the unit on.
///
/// Everything non-hermetic (exe/cwd/home/user lookup, port probe, file write,
/// command spawn) goes through this port, so the suite never runs a real
/// `systemctl`/`launchctl` or touches `~/.config`.
pub trait ServiceHost {
    /// Path of the running `pdo` binary — the unit's `ExecStart`.
    fn current_exe(&self) -> anyhow::Result<PathBuf>;
    /// Current working directory — the unit's `WorkingDirectory` (→ repo_root).
    fn current_dir(&self) -> anyhow::Result<PathBuf>;
    /// Directory holding `node`, or `None` when it isn't on `$PATH`.
    fn node_dir(&self) -> Option<PathBuf>;
    /// `$XDG_CONFIG_HOME`-resolved config home (systemd unit lives under it).
    fn config_home(&self) -> anyhow::Result<PathBuf>;
    /// `$HOME` — launchd plist location + the plist's `HOME` env var.
    fn home(&self) -> anyhow::Result<PathBuf>;
    /// `$USER` — for `loginctl enable-linger $USER`.
    fn user(&self) -> anyhow::Result<String>;
    /// Classify the address the service daemon would occupy.
    fn probe_port(&self, addr: SocketAddr) -> PortState;
    /// Write a unit/plist file, creating parent dirs.
    fn write_file(&self, path: &Path, contents: &str) -> anyhow::Result<()>;
    /// Remove a unit/plist file (uninstall); Ok if already absent.
    fn remove_file(&self, path: &Path) -> anyhow::Result<()>;
    /// Run a side-effect command, capturing its outcome.
    fn run_cmd(&self, program: &str, args: &[&str]) -> anyhow::Result<CmdOutcome>;
}
