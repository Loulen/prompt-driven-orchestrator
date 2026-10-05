//! What the review of a Run needs from the outside: the Run itself (the run
//! context), git (the workspace's adapter), the instance setting that lets an
//! agent resolve a comment (the instance settings, still legacy).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use pdo_run::domain::event::Event;
use pdo_run::domain::state::RunState;

use crate::domain::diff::{FileDiff, StructuredDiff};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A Run as its review reads it.
pub struct ReviewedRun {
    pub events: Vec<Event>,
    pub state: RunState,
    /// The Run's effective repository (ADR-0033): its own, or the daemon's.
    pub repo: PathBuf,
    /// The Run's shared worktree while it exists on disk (#835).
    pub worktree: Option<PathBuf>,
}

/// The run context, as the review reads and writes it.
pub trait ReviewRuns: Send + Sync {
    /// The Run's log and its projection. `Err` (the message) when the log
    /// cannot be read, `Ok(None)` when it holds no started Run.
    fn load<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<Option<ReviewedRun>, String>>;

    /// Append one review event to the Run's log (guarded, stored, published).
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), String>>;

    /// Start the Run's Pipeline Manager if it is not running: `true` when this
    /// call started it. A refusal is `(status, body)`, answered verbatim.
    fn ensure_manager<'a>(
        &'a self,
        run_id: &'a str,
        run: &'a RunState,
    ) -> BoxFuture<'a, Result<bool, (u16, serde_json::Value)>>;

    /// Paste `message` into the manager's session, off the request — after a
    /// grace when `just_started` (the harness has to be reading its input).
    fn hand_to_manager(&self, run_id: &str, message: String, just_started: bool);

    /// The Projets the Run is in (CORE-3): its review comments' Operations
    /// are in them. None for an unknown Run.
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>>;
}

/// A git failure: whether it means an unknown ref or repository (the caller
/// answers `404`), and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFailure {
    pub unknown_revision: bool,
    pub message: String,
}

/// What `git diff <range>` printed.
pub struct RawDiff {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// git, as the review reads a Run's repository.
pub trait ReviewGit: Send + Sync {
    /// `git diff <range> -- . :(exclude).pdo/`; `Err` when git cannot run.
    fn raw_diff(&self, repo: &Path, range: &str) -> Result<RawDiff, String>;

    /// The structured diff of `from` → `to` (three-dot or two-dot).
    fn structured(
        &self,
        repo: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure>;

    /// The structured diff of `from` → the working tree of `worktree`, `to`
    /// being the id reported for the destination.
    fn structured_worktree(
        &self,
        worktree: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure>;

    /// The two-dot diff of one path between two objects.
    fn path_diff(
        &self,
        repo: &Path,
        from: &str,
        to: &str,
        path: &str,
    ) -> Result<Vec<FileDiff>, GitFailure>;

    /// A ref resolved to a commit.
    fn rev_parse(&self, repo: &Path, git_ref: &str) -> Option<String>;

    /// A ref resolved to any object (a commit, or a snapshot tree).
    fn rev_parse_object(&self, repo: &Path, git_ref: &str) -> Option<String>;

    /// The working tree of `worktree` as a tree object, its index untouched.
    fn snapshot_worktree(&self, worktree: &Path) -> Result<String, GitFailure>;

    /// The bytes of `path` at `git_ref`.
    fn file_at_ref(&self, repo: &Path, git_ref: &str, path: &str) -> Result<Vec<u8>, GitFailure>;
}

/// The instance setting `review_agent_can_resolve`, read fresh at each call.
pub trait ReviewSettings: Send + Sync {
    fn agent_can_resolve(&self) -> BoxFuture<'_, bool>;
}
