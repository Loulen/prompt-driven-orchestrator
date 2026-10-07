//! What the run context needs from the outside world. The composition root
//! plugs an adapter into each port; the use cases only ever see the traits.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use pdo_harness::agent_choice::{AgentChoice, ResolvedCombo};
use pdo_skill::domain::delivery::SkippedSkill;
use pdo_skill::domain::selection::{ActiveSkill, SkillRef};
use pdo_workspace::domain::project::Project;
use pdo_workspace::domain::provisioning::{ProvisioningRules, ScopedRules};

use crate::domain::event::Event;
use crate::domain::run_summary::RunSummary;
use crate::domain::state::{CostStat, RunState};

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The append-only event log of every Run, and the Résumé de Run of each
/// (ADR-0084).
pub trait EventStore: Send + Sync {
    /// Record `event` at the end of its Run's log. Refused when the Run has
    /// been forgotten (ADR-0024), in the same step as the write.
    ///
    /// In the same transaction, the Résumé of the event's Run becomes
    /// [`summarize`](crate::domain::run_summary::summarize) of its whole log,
    /// the event included: a refused or failed append changes neither.
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, anyhow::Result<()>>;
    /// Every event of `run_id`, in append order. Empty for an unknown Run.
    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>>;
    /// The id of every Run the log holds, newest first.
    fn run_ids(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>>;
    /// Whether `run_id` was forgotten (ADR-0024): its log is gone for good and
    /// every command on it answers `410`.
    fn is_forgotten<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<bool>>;
    /// Whether a Run was started as a child of `run_id`. `true` when the log
    /// cannot tell: the caller then takes the slow, exact path.
    fn has_children<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, bool>;
    /// Forget `run_id` for good (ADR-0024): tombstone it and purge its log
    /// and its Résumé in one step, so no late writer resurrects it.
    fn forget<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<()>>;
    /// The Résumé of every Run, newest first (ADR-0084).
    fn summaries(&self) -> BoxFuture<'_, anyhow::Result<Vec<RunSummary>>>;
    /// The Résumé of `run_id`; `None` for an unknown Run.
    fn summary<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Option<RunSummary>>>;
    /// Whether the Résumés are those of the log: written under
    /// [`SUMMARY_VERSION`](crate::domain::run_summary::SUMMARY_VERSION) and
    /// up to its last event. `false` before the first rebuild, and after a
    /// daemon that did not keep them wrote to the log.
    fn summaries_current(&self) -> BoxFuture<'_, anyhow::Result<bool>>;
    /// Replace every Résumé by `summaries`, in one step, and mark them
    /// current.
    fn replace_summaries(&self, summaries: Vec<RunSummary>) -> BoxFuture<'_, anyhow::Result<()>>;
}

/// The reads across every Run's log that the downstream contexts aggregate
/// (the Stats cockpit): counts per period, the cohort of Runs started in a
/// window, the Node starts of that cohort, and what a Pipeline's history ran.
/// A window is `[from, to)` on the event's `ts`; under `completed_only` only
/// the Runs that reached `RunCompleted` count (#810). A period label is the
/// event's `ts` formatted with `bucket_format` (a SQLite `strftime` format).
pub trait EventWindows: Send + Sync {
    /// How many events of `kind` fall in each period of the window, periods in
    /// ascending order.
    fn count_by_period<'a>(
        &'a self,
        kind: &'a str,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>>;
    /// The `run_started` events of the window, oldest first, with their period
    /// when `bucket_format` is given.
    fn run_starts<'a>(
        &'a self,
        bucket_format: Option<&'a str>,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<RunStart>>>;
    /// Every `node_started` of the Runs started in the window, by the Run's
    /// start then the event's id, each beside its Run's `run_started`.
    fn node_starts<'a>(
        &'a self,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<NodeStart>>>;
    /// The Node ids that ever started under a Run of Pipeline `pipeline`, the
    /// whole history. A Run's Pipeline is its `pipeline_id`, else its
    /// `pipeline_name`, else `unknown`.
    fn nodes_started_under<'a>(
        &'a self,
        pipeline: &'a str,
        unknown: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<std::collections::BTreeSet<String>>>;
    /// The `run_started` payload of the most recent Run of Pipeline `pipeline`
    /// (same Pipeline rule as [`Self::nodes_started_under`]).
    fn latest_run_start<'a>(
        &'a self,
        pipeline: &'a str,
        unknown: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<serde_json::Value>>>;
}

/// A `[from, to)` window over the events' `ts`.
#[derive(Debug, Clone, Copy)]
pub struct EventWindow<'a> {
    pub from: &'a str,
    pub to: &'a str,
    /// Only the Runs that reached `RunCompleted` (#810).
    pub completed_only: bool,
}

/// One `run_started` of a window.
#[derive(Debug, Clone, PartialEq)]
pub struct RunStart {
    pub run_id: String,
    pub ts: String,
    /// The period label, when one was asked for.
    pub bucket: Option<String>,
    /// The payload as stored (`Null` when absent or unreadable).
    pub payload: serde_json::Value,
}

/// One `node_started` of a Run started in a window, beside that Run's start.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeStart {
    /// The period label of the Run's start.
    pub bucket: String,
    pub run_id: String,
    /// The Run's `run_started` timestamp and payload.
    pub run_ts: String,
    pub run_payload: serde_json::Value,
    pub event_id: Option<i64>,
    pub node_id: Option<String>,
    pub iter: Option<i64>,
    pub payload: serde_json::Value,
}

/// What a Run's detail reads live from disk on top of its projection: its
/// lines-of-code delta, its cost, the run-scoped pipeline's graph. Never
/// fails: an unreadable source leaves its field as the projection had it.
pub trait LiveFacts: Send + Sync {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()>;
    /// The Run's cost alone, without the lines-of-code walk: what the children
    /// list shows. `None` when it is unavailable (the surfaces' "—", never $0).
    fn cost<'a>(
        &'a self,
        run: &'a RunState,
        events: &'a [Event],
    ) -> BoxFuture<'a, Option<CostStat>>;
}

/// The Pipeline Manager's sessions, as a Run's detail observes them.
pub trait ManagerSessions: Send + Sync {
    /// Whether the manager of `run_id` has a live session right now.
    fn exists(&self, run_id: &str) -> bool;
}

/// The instance settings a Run reads, as the settings context holds them. Read
/// fresh at every seam that needs them, so a `PUT /settings` takes effect on
/// the next spawn or sweep with no restart (ADR-0015).
#[derive(Debug, Clone, Default)]
pub struct InstanceDefaults {
    /// The stored session cap, before the environment tier.
    pub session_cap: Option<i64>,
    /// The stored instance-wide model, before the environment tier.
    pub default_model: Option<String>,
    /// The stored instance-wide harness, before the environment tier.
    pub default_harness: Option<String>,
    /// The stored per-harness default models.
    pub default_harness_model: BTreeMap<String, String>,
    /// The stored turn-end auto-completion (`Some(0)` / `Some(1)`).
    pub autocomplete_turn_end: Option<i64>,
    /// The instance tier of the auto-fail preference, resolved `stored → env →
    /// false`.
    pub auto_fail: bool,
    /// The instance tier of the agentic-profile union (ADR-0057).
    pub agent_choice: Option<AgentChoice>,
    /// The instance tier of the skills selection (ADR-0062).
    pub skills: Vec<SkillRef>,
    /// The stored attachment budget of a `POST /runs`, in MB, before the
    /// environment tier.
    pub max_attachments_mb: Option<i64>,
    /// The stored default isolation of a new Run, before the environment tier.
    pub default_sandbox: Option<String>,
    /// The stored default of the manager's auto-naming (`Some(0)` / `Some(1)`).
    pub default_auto_name: Option<i64>,
    /// Whether a new Run spawns its Pipeline Manager on its own, resolved
    /// `stored → env → false` by the settings side.
    pub manager_enabled: bool,
    /// The stored agentic profile the manager is pinned to, if any.
    pub manager_profile: Option<String>,
}

/// The instance settings and the agentic profiles, which the settings context
/// owns. Never fails: an unreadable store degrades each tier to transparent.
pub trait InstanceSettings: Send + Sync {
    /// `None` when the settings cannot be read.
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>>;
    /// The profiles, as one atomic snapshot (ADR-0057 ¶4); empty when they
    /// cannot be read.
    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>>;
    /// The agentic profile named `name`, case-insensitively: its combo, `None`
    /// when no profile has that name, `Err` when the profiles cannot be read.
    fn profile_named<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<ResolvedCombo>, String>>;
}

/// The page mounts a Run's agents published (the platform context), which an
/// archive releases.
pub trait RunPages: Send + Sync {
    /// Hold every page-mount mutation off until the guard drops: an archive
    /// holds it across the whole teardown of the Run and its descendants.
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>>;
    /// Release the page mounts of `run_id`.
    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// The pipeline watcher: a new Run's directory is registered with it, so an
/// edit of the run-scoped pipeline on disk is announced (the pipeline context).
pub trait RunDirWatch: Send + Sync {
    fn watch(&self, run_dir: &Path);
}

/// The skill context, as a node's spawn reads and delivers it.
pub trait NodeSkills: Send + Sync {
    /// The bank's `id → name`; empty when it cannot be read.
    fn bank_names(&self) -> BoxFuture<'_, BTreeMap<String, String>>;
    /// Deliver `skills` into the tree a node works in; the skills skipped (a
    /// versioned homonym) are handed back.
    fn deliver(
        &self,
        worktree: &Path,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>>;
    /// Take back what a delivery wrote for `run_id` in `repo_root` (its
    /// exclusions, its snapshot).
    fn withdraw(&self, repo_root: &Path, run_id: &str) -> anyhow::Result<()>;
}

/// The Projets a Run's repositories belong to (the workspace context).
pub trait Projects: Send + Sync {
    /// The Projet that owns `path`, compared verbatim (ADR-0033).
    fn owner_of<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>>;
    /// The provisioning rules a Run in `repository` freezes: the instance's,
    /// the Projet's, then `run`'s own (ADR-0063).
    fn provisioning_rules<'a>(
        &'a self,
        repository: &'a str,
        run: ProvisioningRules,
    ) -> BoxFuture<'a, Result<Vec<ScopedRules>, String>>;
}

/// The declared wait of the session context, as a completion declares it for
/// the agent.
pub trait DeclaredWaits: Send + Sync {
    /// The agent is done but its completion waits for the operator's click.
    /// Best-effort: a refusal is logged, never surfaced.
    fn on_completion_not_released<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, ()>;
}
