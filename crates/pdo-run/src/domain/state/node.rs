//! A node of a Run, as the projection folds it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{CostForm, IterationInfo, NodeStatus};

/// The TypeScript shape of one entry of [`NodeState::frontmatter_violations`]: the
/// payload is copied verbatim from the refusal event, built by the outputs
/// validator as `{ port, field, reason }`.
#[derive(TS)]
#[ts(export, rename = "FrontmatterViolation")]
#[allow(dead_code)]
struct FrontmatterViolationWire {
    port: String,
    field: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "ProjectedNodeState")]
pub struct NodeState {
    pub node_id: String,
    /// #616/ADR-0046: the harness this node's session was **frozen** on at spawn,
    /// read from the `NodeStarted` payload — so the Run view shows, per node, what
    /// actually ran (the run-level default lives on [`RunState::harness`]). `None`
    /// for a node that never opened a `NodeStarted` (a pure skip) or a pre-#616
    /// snapshot; `skip_serializing_if` keeps the wire byte-identical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub harness: Option<String>,
    /// #653/ADR-0060: the isolation this NodeRun was **frozen** on at spawn,
    /// read from the `NodeStarted` payload. This — not the current document — is
    /// what says where the live iteration works, so editing the graph mid-run
    /// never moves a running node between worktrees. `None` for a node that
    /// never opened a `NodeStarted`, for a structural node, or for a pre-#653
    /// snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub isolated_worktree: Option<bool>,
    /// #669/ADR-0062: the **skills actifs** this NodeRun was frozen with at
    /// spawn (union of the four tiers, each with its origin), read from the
    /// `NodeStarted` payload. `None` for a node that never started, a `script`
    /// node (no agent) or a pre-#669 event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skills: Option<Vec<pdo_skill::domain::selection::ActiveSkill>>,
    /// #669: selected ids the bank no longer knew at spawn — the node ran
    /// without them (a warning, never a failure). Absent when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_skills: Vec<pdo_skill::domain::selection::MissingSkill>,
    /// #672: skills promised to this NodeRun that could not be written into its
    /// worktree (versioned homonym, occupied path, content gone) — the node ran
    /// without them. Absent when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_skills: Vec<pdo_skill::domain::delivery::SkippedSkill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost: Option<NodeCost>,
    pub status: NodeStatus,
    pub iter: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
    /// Why the node was **auto-skipped** as structurally unreachable (#620),
    /// lifted from the skip event's `reason` payload so it reads at node level and
    /// not only in the log. Present only when `status == Skipped`; a skip is NOT a
    /// failure, so it stays out of `failure_reason` (which the UI paints red).
    /// Absent on every other status — `skip_serializing_if` keeps the wire shape
    /// byte-identical for a non-skipped node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skip_reason: Option<String>,
    #[serde(default)]
    pub iterations: Vec<IterationInfo>,
    #[serde(default)]
    pub frontmatter_retries: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Vec<FrontmatterViolationWire>")]
    pub frontmatter_violations: Vec<serde_json::Value>,
    /// #490: declared output ports the validator found empty, from a `script`
    /// node's fail-fast branch (`ValidationError::MissingOutputs`). Mutually
    /// exclusive with `frontmatter_violations` by construction — `ValidationError`
    /// is an exclusive-or, so no discriminator field is needed.
    ///
    /// Without a home, a `script` node that failed on a *missing output* rendered
    /// the red banner with an empty list: the daemon computed the evidence and the
    /// projector threw it away. `skip_serializing_if` mirrors its neighbour, so the
    /// field is **absent** from every pre-#490 response and the wire shape is
    /// byte-identical for any non-`script` failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_outputs: Vec<String>,
    /// #654/ADR-0060: what this NodeRun **delivered** onto the Run's branch — the
    /// two tips its delivery moved the branch between, so `git diff before after`
    /// is exactly its contribution.
    ///
    /// Present for any NodeRun that delivered changes, isolated or not; absent for
    /// one that delivered nothing (no commit was written) and on any pre-#654 log.
    /// It is the presence of *changes*, never the node's type or isolation, that
    /// makes a per-node diff answerable — which is the whole point of recording it
    /// here rather than re-deriving it from a `pdo/sub-*` branch that a
    /// non-isolated NodeRun does not have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivery: Option<NodeDelivery>,
    /// #588 / ADR-0069: why this node is `AwaitingUser` — the declared wait's
    /// cause and message, or the derived « a child run is awaiting you » overlay
    /// ([`crate::domain::child_awaiting`]). Present only while `status == AwaitingUser`;
    /// cleared by `NodeResumed`, `NodeCompletionReleased` and every re-spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub awaiting: Option<AwaitingInfo>,
}

/// The declared wait of a node (#588 / ADR-0069), as the surfaces read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AwaitingInfo {
    /// `declared` (`pdo wait-user`), `completion_not_released` (a refused
    /// `pdo complete` on an unreleased interactive node), `children_pending`
    /// (the orchestrator binding marker, ADR-0064) or `child_awaiting` (derived
    /// on read from an awaiting child run, never written to this run's log).
    pub cause: String,
    /// The prose the banner shows: the agent's question (capped at
    /// [`pdo_session::domain::declared_wait::MESSAGE_MAX_CHARS`] by the CLI), the daemon's
    /// sentence on a refused completion, or « child run <name> is awaiting you ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
    /// When the wait was declared (the event's timestamp) — the banner's
    /// « waiting N min ».
    pub since: String,
    /// The awaiting child run, for the `child_awaiting` overlay only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub child_run_id: Option<String>,
}

/// Cause slugs of [`AwaitingInfo`].
pub const AWAITING_CAUSE_DECLARED: &str = "declared";
pub const AWAITING_CAUSE_COMPLETION_NOT_RELEASED: &str = "completion_not_released";
pub const AWAITING_CAUSE_CHILD_AWAITING: &str = "child_awaiting";

/// The two Run-branch tips one delivery moved between (#654 / ADR-0060).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeDelivery {
    /// The Run branch's tip before the delivery.
    pub before: String,
    /// The Run branch's tip after it. Never equal to `before` — a delivery that
    /// moved nothing records no event at all.
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeCost {
    pub usd: Option<f64>,
    pub form: Option<CostForm>,
    /// True when every contribution is a reported cost **already in dollars**
    /// (constant 1.0, `pi`): rendered without `~` (#707). See
    /// [`HarnessCost::reported_in_usd`].
    #[serde(default)]
    pub reported_in_usd: bool,
    pub partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unpriced_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_reasons: Vec<String>,
    pub executions: i64,
    pub readable_executions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StartNodeInfo {
    pub input_path: String,
    pub started_at: String,
    pub target_node_ids: Vec<String>,
    /// Filenames of the images uploaded alongside the text prompt (stored in
    /// `_input/`). Empty when the run was launched without images. Surfaced on
    /// the Start node and in the Start inspector (issue #145).
    #[serde(default)]
    pub input_images: Vec<String>,
    /// Filenames of the NON-image files uploaded alongside the prompt (#779),
    /// also stored in `_input/`. Empty for a run launched without files.
    #[serde(default)]
    pub input_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EndPortStatus {
    pub port_name: String,
    pub status: String,
    pub reason: Option<String>,
    pub fired_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EndNodeInfo {
    pub id: String,
    pub ports: Vec<EndPortStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeResolverInfo {
    pub status: NodeStatus,
    pub conflicting_node_id: String,
    pub iter: i64,
    pub session_name: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
}
