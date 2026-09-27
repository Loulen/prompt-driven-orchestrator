//! What the run context needs from the outside world. The composition root
//! plugs an adapter into each port; the use cases only ever see the traits.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use axum::response::Response;
use pdo_harness::agent_choice::{AgentChoice, ResolvedCombo};
use pdo_skill::domain::delivery::SkippedSkill;
use pdo_skill::domain::selection::{ActiveSkill, SkillRef};
use pdo_workspace::domain::project::Project;

use crate::domain::event::Event;
use crate::domain::state::RunState;

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The append-only event log of every Run.
pub trait EventStore: Send + Sync {
    /// Record `event` at the end of its Run's log. Refused when the Run has
    /// been forgotten (ADR-0024), in the same step as the write.
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
}

/// What a Run's detail reads live from disk on top of its projection: its
/// lines-of-code delta, its cost, the run-scoped pipeline's graph. Never
/// fails: an unreadable source leaves its field as the projection had it.
pub trait LiveFacts: Send + Sync {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()>;
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
}

/// The instance settings and the agentic profiles, which the settings context
/// owns. Never fails: an unreadable store degrades each tier to transparent.
pub trait InstanceSettings: Send + Sync {
    /// `None` when the settings cannot be read.
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>>;
    /// The profiles, as one atomic snapshot (ADR-0057 ¶4); empty when they
    /// cannot be read.
    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>>;
}

/// The Run's surfaces that still live in the legacy crate, which the Pipeline
/// Manager's commands reach. Each answers the HTTP response the surface gives,
/// verbatim (ADR-0009: the command surface forwards it unchanged).
pub trait RunSurfaces: Send + Sync {
    /// Archive a terminal Run (`cleanup_run`).
    fn cleanup<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Response>;
    /// Archive a terminal Run and start its twin (`retry_all`).
    fn retry_all<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Response>;
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
}

/// The Projets a Run's repositories belong to (the workspace context).
pub trait Projects: Send + Sync {
    /// The Projet that owns `path`, compared verbatim (ADR-0033).
    fn owner_of<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>>;
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
