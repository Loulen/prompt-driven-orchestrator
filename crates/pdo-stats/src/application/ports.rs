//! What the stats context needs from the outside world. The composition root
//! plugs an adapter into each port; the use cases only ever see the traits.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use pdo_platform::domain::audit::NewAuditEntry;
use pdo_run::application::ports::{EventWindow, NodeStart, RunStart};
use pdo_run::domain::event::Event;
use pdo_run::domain::prices::{Price, PriceTable};
use pdo_run::domain::run_cost::CostContribution;
use pdo_sandbox::adapters::staging::stores::HarnessStores;
use pdo_workspace::domain::project::Project;

use crate::domain::absorption::{AbsorptionRow, Named, Scope};
use crate::domain::overview::TriggersCreatedRuns;

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The Runs' event log, as the run context serves it: its cross-Run reads and
/// one Run's events. Never the table itself.
pub trait RunEvents: Send + Sync {
    /// How many events of `kind` per period of the window.
    fn count_by_period<'a>(
        &'a self,
        kind: &'a str,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>>;
    /// The Runs started in the window, oldest first.
    fn run_starts<'a>(
        &'a self,
        bucket_format: Option<&'a str>,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<RunStart>>>;
    /// Every `node_started` of the Runs started in the window.
    fn node_starts<'a>(
        &'a self,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<NodeStart>>>;
    /// The Node ids that ever started under raw Pipeline key `pipeline`.
    fn nodes_started_under<'a>(
        &'a self,
        pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<BTreeSet<String>>>;
    /// The `run_started` payload of the most recent Run of `pipeline`.
    fn latest_run_start<'a>(
        &'a self,
        pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<serde_json::Value>>>;
    /// Every event of one Run, in append order.
    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>>;
    /// The child Runs of the whole log — a `run_started` whose parent the log
    /// still holds — whatever their own start: what « Roll up child runs »
    /// walks from each root (ADR-0101).
    fn child_run_starts(&self) -> BoxFuture<'_, anyhow::Result<Vec<RunStart>>>;
}

/// The Triggers' fires, as the trigger context holds them.
pub trait TriggerFires: Send + Sync {
    /// The fires of the window per Pipeline key; a fire whose Trigger was
    /// deleted counts under `"(deleted trigger)"`.
    fn by_pipeline<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>>;
    /// The « triggers that created a Run » KPI of the window.
    fn created_runs<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TriggersCreatedRuns>>;
}

/// The Projets, as the workspace context holds them.
pub trait Projects: Send + Sync {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, String>>;
}

/// The absorptions of the instance (ADR-0077), stored flat, one level. Every
/// write is one transaction.
pub trait AbsorptionStore: Send + Sync {
    /// Every stored member row.
    fn list(&self) -> BoxFuture<'_, anyhow::Result<Vec<AbsorptionRow>>>;
    /// Create or extend an absorption in `scope`, flattened on write (ADR-0077
    /// §3). A Pipeline Combine carries the members' Node absorptions over to
    /// the absorbent's scope (#892).
    fn combine<'a>(
        &'a self,
        scope: Scope<'a>,
        absorbent: &'a Named,
        members: &'a [Named],
        origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
    /// The same Combine, once per raw scope of `covered` (the frozen scope of
    /// an effort or couple absorption, ADR-0078), in one transaction.
    fn combine_frozen<'a>(
        &'a self,
        dimension: &'a str,
        covered: &'a [Named],
        absorbent: &'a Named,
        members: &'a [Named],
        origin: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
    /// Take `member` out of its absorption in `dimension` × `scope`, and the
    /// Node members `released` out of the Node absorptions scoped to
    /// `released_scope` with it, in one transaction.
    fn remove<'a>(
        &'a self,
        dimension: &'a str,
        scope: &'a str,
        member: &'a str,
        released_scope: &'a str,
        released: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<()>>;
}

/// Where the files of one Run live: its transcripts, its repository, the
/// harnesses' reported stores.
#[derive(Debug, Clone)]
pub struct RunFilesRoots {
    pub claude_root: PathBuf,
    pub repo_root: PathBuf,
    pub stores: HarnessStores,
}

/// The Runs' files on disk, as the harnesses wrote them.
pub trait RunFiles: Send + Sync {
    /// Where the files of `run_id` are read from, from its `run_started`
    /// payload (its repository, its sandbox).
    fn roots(&self, run_id: &str, payload: &serde_json::Value) -> RunFilesRoots;
    /// The Run's cost contributions, memoized per Run (ADR-0029).
    fn cost_contributions(
        &self,
        events: &[Event],
        roots: &RunFilesRoots,
        run_id: &str,
        prices: &PriceTable,
    ) -> Vec<CostContribution>;
    /// What changes when the Run's events or files change: the memo key of a
    /// whole-cohort read.
    fn footprint(&self, events: &[Event], roots: &RunFilesRoots, run_id: &str) -> u64;
    /// The price table in force (the HOST home's tiers, resolved per read).
    fn prices(&self) -> PriceTable;
}

/// The transcripts of the harnesses' sessions, read for the Performance fold.
pub trait SessionTranscripts: Send + Sync {
    /// Whether a harness's whole source at `root` can be listed. An absent
    /// root is fine (no session written yet); the error names the cause.
    fn source_listable(&self, root: &Path) -> Result<(), String>;
    /// The text of one main session, by the harness's own resolution (with
    /// no session id, a harness that can falls back on its latest session).
    fn main_session(
        &self,
        harness: &str,
        root: &Path,
        working_dir: &Path,
        session_id: Option<&str>,
    ) -> Option<String>;
    /// The subagent transcripts of one main session: `(file stem, text)`.
    fn subagents(
        &self,
        harness: &str,
        root: &Path,
        working_dir: &Path,
        session_id: &str,
    ) -> Vec<(String, String)>;
    /// The Claude session ids (`.jsonl` stems) in `working_dir`'s project
    /// directory under `claude_root`, with that directory; `None` when it
    /// cannot be listed.
    fn claude_sessions_in(
        &self,
        claude_root: &Path,
        working_dir: &Path,
    ) -> Option<(PathBuf, Vec<String>)>;
    /// The text of one file.
    fn read(&self, path: &Path) -> Option<String>;
}

/// The price table's files: where they are, and the atomic write of the
/// fetched tier.
pub trait PriceFiles: Send + Sync {
    /// The host home the price files live under, or why there is none.
    fn home(&self) -> Result<PathBuf, String>;
    /// The table in force under `home`.
    fn load(&self, home: &Path) -> PriceTable;
    /// `(models.yaml, fetched.json)` under `home`.
    fn paths(&self, home: &Path) -> (PathBuf, PathBuf);
    /// Write the fetched tier (never an empty one).
    fn write_fetched(
        &self,
        path: &Path,
        source: &str,
        fetched_at: &str,
        rows: &BTreeMap<String, Price>,
    ) -> std::io::Result<()>;
}

/// The price source (ADR-0034): the one egress.
pub trait PriceSource: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<String, String>>;
}

/// The audit journal of the platform context (ADR-0044).
pub trait AuditJournal: Send + Sync {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()>;
}

/// What every connected client is told.
pub trait Announcements: Send + Sync {
    /// The price table moved: `rows` rows from the source.
    fn cost_prices_synced(&self, rows: usize);
}
