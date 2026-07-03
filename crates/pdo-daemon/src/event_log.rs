//! Run state as an event-sourced projection.
//!
//! A Run's canonical state lives in its append-only event log; [`project`] folds
//! that log into a [`RunState`]. The fold is a thin dispatch loop that routes
//! each event, by concern, to exactly one per-concern sub-applier (`apply_run_event`,
//! `apply_node_event`, `apply_switch_event`, `apply_loop_event`,
//! `apply_foreach_event`, `apply_merge_event`, `apply_pipeline_event`,
//! `apply_command_event`), then runs a single [`finalize`] reconciliation pass.
//! The dispatch `match` is exhaustive over every [`EventKind`] with no wildcard,
//! so adding a variant fails to compile until it is routed (#238).
//!
//! `project` is pure and MUST NOT panic: besides every read, it also runs inside
//! `append_event` (before the transition guard) to compute the current state fed
//! to that guard, so a panic here would break event appends — hence each
//! applier's inner match ends in a silent `_ => {}` rather than `unreachable!()`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halt_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_clause: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBrief {
    pub name: String,
    pub side: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub node_type: String,
    pub view_x: Option<f64>,
    pub view_y: Option<f64>,
    pub inputs: Vec<PortBrief>,
    pub outputs: Vec<PortBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStarted,
    NodeStarted,
    /// The node is ready to run but throttled by the global session cap: it
    /// holds no tmux session yet and waits for an admission slot (#159).
    NodeWaiting,
    NodeAwaitingUser,
    NodeCompleted,
    NodeFailed,
    MergeConflictDetected,
    MergeResolverStarted,
    MergeResolverCompleted,
    MergeResolverFailed,
    SwitchRouted,
    LoopIterStarted,
    LoopBreakReceived,
    LoopMaxReached,
    LoopDone,
    FrontmatterRetryPending,
    ForEachStarted,
    ForEachEmpty,
    ForEachBreakReceived,
    ForEachDone,
    NodeStopped,
    NodeAutoCompleted,
    NodeStale,
    NodeInvalidated,
    /// Informational (#290): a node's Claude Code session is blocked on the
    /// usage-limit interactive menu (host-level; session alive, no progress).
    /// Behaviour-preserving no-op in projection — the node stays Running;
    /// recovery is deferred (Slice 2/3). Wire form: `"node_blocked_on_limit"`.
    NodeBlockedOnLimit,
    PipelineLint,
    PipelineModified,
    RunCompleted,
    RunFailed,
    /// Graceful no-op (#245): the run fired but there was legitimately nothing
    /// to do (e.g. an auto-issue selector found its eligible pool emptied
    /// between guard-eval and node-run). A distinct terminal status from
    /// `RunFailed` so honest history is not polluted with spurious failures.
    RunSkipped,
    RunHalted,
    RunPaused,
    RunResumed,
    RunArchived,
    RunRenamed,
    CommandIssued,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Option<i64>,
    pub run_id: String,
    pub ts: String,
    pub kind: EventKind,
    pub node_id: Option<String>,
    pub iter: Option<i64>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    AwaitingUser,
    Completed,
    Failed,
    /// Graceful no-op terminal state (#245): the run fired but had nothing to
    /// do. Terminal and non-`is_live`, distinct from `Completed` (did work) and
    /// `Failed` (genuine error), so "fired but nothing to do" stays honest.
    Skipped,
    Halted,
    Paused,
    Archived,
}

impl RunStatus {
    /// A Run is "live" while it is `Running`, `AwaitingUser`, or `Paused`. While
    /// live, its session-holding nodes still consume an admission slot and a new
    /// trigger fire is blocked by an overlapping run.
    ///
    /// `Completed`/`Failed`/`Skipped`/`Halted`/`Archived` are terminal: such a
    /// run spawns no new work, so its nodes hold no live session (#215).
    /// `Skipped` is a graceful no-op (#245); `Halted` is terminal-but-resumable
    /// but, while halted, holds nothing either.
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            RunStatus::Running | RunStatus::AwaitingUser | RunStatus::Paused
        )
    }

    /// A Run is terminal exactly when it is not live — the total complement of
    /// [`is_live`](Self::is_live). `{Completed, Failed, Skipped, Halted,
    /// Archived}`. `Paused` is NOT terminal (it is live: holds a slot, blocks
    /// overlap, is resumable). Defined as `!is_live()` so the two stay mutually
    /// exclusive and exhaustive and a future variant cannot silently fall
    /// between them.
    ///
    /// NOTE: several call sites use a *different* terminality set on purpose
    /// (boot recovery omits `Skipped`; `retry_all` omits `Archived`; the
    /// delete-pipeline guard is a third "active run" predicate). Those are
    /// deliberately NOT migrated onto this method — see the F1/F2/F3 follow-ups
    /// in the #237 plan.
    pub fn is_terminal(&self) -> bool {
        !self.is_live()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    /// Throttled by the global session cap: the node is ready to run but no
    /// admission slot is free, so it has *not* spawned a tmux session yet. It
    /// transitions to `Running` once a slot frees (admission control, #159).
    Waiting,
    Running,
    AwaitingUser,
    Completed,
    Failed,
    Stopped,
    Stale,
}

impl NodeStatus {
    /// Whether a node in this status currently holds a live NodeRun tmux
    /// session, and therefore consumes a global admission slot.
    /// `{Running, AwaitingUser}` (an interactive node keeps its tmux session
    /// attachable indefinitely). EXCLUDES `Waiting`: a throttled node is ready
    /// to run but has *not* spawned a session yet, so it holds no slot (#159).
    pub fn holds_session(&self) -> bool {
        matches!(self, NodeStatus::Running | NodeStatus::AwaitingUser)
    }

    /// Whether a node in this status can still drive the run forward, so its
    /// presence suppresses a silent-stall verdict (#214).
    /// `{Running, Waiting, AwaitingUser}`. INCLUDES `Waiting` (a throttled node
    /// will spawn and progress as soon as an admission slot frees) — this is
    /// the load-bearing difference from [`holds_session`](Self::holds_session),
    /// which excludes `Waiting`. Collapsing the two would falsely declare a
    /// throttled-but-healthy run stalled (CONTEXT.md, § Réconciliation au
    /// niveau Run).
    pub fn can_progress(&self) -> bool {
        matches!(
            self,
            NodeStatus::Running | NodeStatus::Waiting | NodeStatus::AwaitingUser
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub iter: i64,
    pub status: NodeStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub node_id: String,
    pub status: NodeStatus,
    pub iter: i64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub iterations: Vec<IterationInfo>,
    #[serde(default)]
    pub frontmatter_retries: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frontmatter_violations: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartNodeInfo {
    pub input_path: String,
    pub started_at: String,
    pub target_node_ids: Vec<String>,
    /// Filenames of the images uploaded alongside the text prompt (stored in
    /// `_input/`). Empty when the run was launched without images. Surfaced on
    /// the Start node and in the Start inspector (issue #145).
    #[serde(default)]
    pub input_images: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndPortStatus {
    pub port_name: String,
    pub status: String,
    pub reason: Option<String>,
    pub fired_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndNodeInfo {
    pub id: String,
    pub ports: Vec<EndPortStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResolverInfo {
    pub status: NodeStatus,
    pub conflicting_node_id: String,
    pub iter: i64,
    pub session_name: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
    pub loop_node_id: String,
    pub current_iter: i64,
    pub max_iter: i64,
    pub break_received: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForEachState {
    pub foreach_node_id: String,
    pub total_items: i64,
    pub break_received: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchState {
    pub switch_node_id: String,
    pub chosen_branch: String,
    pub evaluated_at: String,
}

/// Lines-of-code delta for a Run, derived live from `git diff --numstat` of the
/// run branch against its fork point (issue #100). Live-only: it is **not**
/// snapshotted into the event log (J2), so once the run branch is cleaned up it
/// becomes uncomputable and the field is dropped (`None` → UI shows "—").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocStat {
    pub insertions: u64,
    pub deletions: u64,
    pub files_changed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub status: RunStatus,
    pub pipeline_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub input: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub nodes: HashMap<String, NodeState>,
    #[serde(default)]
    pub edges: Vec<EdgeInfo>,
    #[serde(default)]
    pub node_defs: Vec<NodeDefInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_node: Option<StartNodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_node: Option<EndNodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_resolver: Option<MergeResolverInfo>,
    #[serde(default)]
    pub loop_states: HashMap<String, LoopState>,
    #[serde(default)]
    pub foreach_states: HashMap<String, ForEachState>,
    #[serde(default)]
    pub switch_states: HashMap<String, SwitchState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    /// Provenance: the id of the Trigger that created this Run, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
    /// Cumulative count of `NodeStarted` events for this Run — i.e. how many
    /// Claude Code NodeRun sessions it spawned (issue #100). A **raw** count,
    /// not deduplicated by `(node, iter)`: a legal re-spawn at the same
    /// `(node, iter)` (restart/recovery) counts again, so this is always ≥ the
    /// number of distinct iterations shown. The Pipeline Manager emits no
    /// `NodeStarted`, so it is excluded by construction.
    #[serde(default)]
    pub sessions_spawned: u64,
    /// Lines changed for the Run (issue #100). `None` (not `Some(0)`) when the
    /// run branch is gone (archived/cleaned) — the UI renders "—" vs "0".
    /// Derived on read, never persisted; see [`LocStat`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<LocStat>,
}

impl RunState {
    pub fn new(run_id: String, pipeline_name: String) -> Self {
        Self {
            run_id,
            status: RunStatus::Running,
            pipeline_name,
            name: None,
            input: None,
            started_at: None,
            completed_at: None,
            nodes: HashMap::new(),
            edges: Vec::new(),
            node_defs: Vec::new(),
            start_node: None,
            end_node: None,
            merge_resolver: None,
            loop_states: HashMap::new(),
            foreach_states: HashMap::new(),
            switch_states: HashMap::new(),
            target_repo: None,
            source_branch: None,
            triggered_by: None,
            sessions_spawned: 0,
            loc: None,
        }
    }

    /// Status of `node_id` in this run's projection, if the node exists.
    ///
    /// Borrows (`NodeStatus` is `Clone`-not-`Copy`); use for status-only reads
    /// where the whole [`NodeState`] is not needed.
    pub fn node_status(&self, node_id: &str) -> Option<&NodeStatus> {
        self.nodes.get(node_id).map(|n| &n.status)
    }

    /// The latest `Completed` iteration of `node_id`, if any (#210).
    ///
    /// History-max over `Completed` iterations (failed/stopped iters are
    /// quarantined — their artifacts stay on disk but are never resolvable as
    /// inputs), falling back to the head `iter` when the head status is
    /// `Completed` but no per-iteration history exists (legacy states). This is
    /// the single home for the rule formerly duplicated as a free fn in
    /// `input_resolution`.
    pub fn latest_completed_iter(&self, node_id: &str) -> Option<i64> {
        let node = self.nodes.get(node_id)?;
        let from_history = node
            .iterations
            .iter()
            .filter(|it| it.status == NodeStatus::Completed)
            .map(|it| it.iter)
            .max();
        from_history.or_else(|| (node.status == NodeStatus::Completed).then_some(node.iter))
    }

    /// True iff `node_ids` is non-empty AND every id resolves to a node whose
    /// status is `Completed`.
    ///
    /// Completed-only: `Failed`/`Stopped`/`Stale`/`Skipped` do NOT count (those
    /// are handled by the stall / fail-fast paths). A never-spawned id (no
    /// `NodeState`) counts as not-done. An empty set yields `false`, NOT
    /// vacuous-true: a run with no expected nodes is not "all done" (preserving
    /// the original `!is_empty()` guard).
    ///
    /// The authoritative node set is the caller's (`pipeline.nodes` at the
    /// completion/stall sites, the runtime `expected_node_ids` at the
    /// node-done sites) — `RunState` owns neither, so it receives the ids.
    pub fn all_nodes_completed(&self, node_ids: &[String]) -> bool {
        !node_ids.is_empty()
            && node_ids
                .iter()
                .all(|id| self.node_status(id) == Some(&NodeStatus::Completed))
    }
}

fn entry_node_ids(edges: &[EdgeInfo], node_defs: &[NodeDefInfo]) -> Vec<String> {
    let start_id = node_defs
        .iter()
        .find(|n| n.node_type == "start")
        .map(|n| n.id.as_str());

    if let Some(start_id) = start_id {
        edges
            .iter()
            .filter(|e| e.source_node == start_id)
            .map(|e| e.target_node.clone())
            .collect()
    } else {
        let nodes_with_unconditional_incoming: HashSet<&str> = edges
            .iter()
            .filter(|e| e.when_clause.is_none())
            .map(|e| e.target_node.as_str())
            .collect();

        node_defs
            .iter()
            .filter(|n| !nodes_with_unconditional_incoming.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }
}

fn upsert_iteration(iterations: &mut Vec<IterationInfo>, new: IterationInfo) {
    if let Some(existing) = iterations.iter_mut().find(|i| i.iter == new.iter) {
        existing.status = new.status;
        if new.started_at.is_some() {
            existing.started_at = new.started_at;
        }
        if new.completed_at.is_some() {
            existing.completed_at = new.completed_at;
        }
    } else {
        iterations.push(new);
    }
}

pub fn project(events: &[Event]) -> Option<RunState> {
    if events.is_empty() {
        return None;
    }

    let run_id = events[0].run_id.clone();
    let mut state = RunState::new(run_id, String::new());

    for event in events {
        match event.kind {
            EventKind::RunStarted
            | EventKind::RunCompleted
            | EventKind::RunFailed
            | EventKind::RunSkipped
            | EventKind::RunHalted
            | EventKind::RunPaused
            | EventKind::RunResumed
            | EventKind::RunRenamed
            | EventKind::RunArchived => apply_run_event(&mut state, event),

            EventKind::NodeWaiting
            | EventKind::NodeStarted
            | EventKind::NodeCompleted
            | EventKind::NodeAutoCompleted
            | EventKind::NodeAwaitingUser
            | EventKind::NodeFailed
            | EventKind::NodeStopped
            | EventKind::NodeStale
            | EventKind::NodeInvalidated
            | EventKind::FrontmatterRetryPending => apply_node_event(&mut state, event),

            EventKind::MergeConflictDetected
            | EventKind::MergeResolverStarted
            | EventKind::MergeResolverCompleted
            | EventKind::MergeResolverFailed => apply_merge_event(&mut state, event),

            EventKind::SwitchRouted => apply_switch_event(&mut state, event),

            EventKind::LoopIterStarted
            | EventKind::LoopBreakReceived
            | EventKind::LoopMaxReached
            | EventKind::LoopDone => apply_loop_event(&mut state, event),

            EventKind::ForEachStarted
            | EventKind::ForEachEmpty
            | EventKind::ForEachBreakReceived
            | EventKind::ForEachDone => apply_foreach_event(&mut state, event),

            EventKind::PipelineLint | EventKind::PipelineModified => {
                apply_pipeline_event(&mut state, event)
            }

            // #290: informational only — the node stays in its current status
            // (Running). Behaviour-preserving no-op, exactly like `PipelineLint`;
            // recovery/unblocking is deferred (Slice 2/3). No node/run state touched.
            EventKind::NodeBlockedOnLimit => {}

            EventKind::CommandIssued => apply_command_event(&mut state, event),
        }
    }

    finalize(&mut state);

    Some(state)
}

// ── Per-concern sub-appliers (#238) ──────────────────────────────────────────
//
// `project()` routes each event to exactly one applier by concern; every applier
// takes `(&mut RunState, &Event)` and folds that one event into the state. Each
// multi-variant applier runs a focused inner `match event.kind` over only its
// own subset and ends in a silent `_ => {}` — the appliers MUST NOT panic, since
// `project()` also runs inside `append_event` (before the transition guard), so
// a panic here would break event appends, not just reads. Arm bodies are moved
// verbatim from the former monolithic match; the incident comments they carry
// (#221, #196/#212, #199, #245, #159, #100) are load-bearing — do not reword.

/// Run-lifecycle events: start (bootstrap pipeline/edges/node-defs/start+end
/// nodes), the terminal transitions (completed/failed/skipped/halted), the
/// resumable pause/resume pair, rename, and archive.
fn apply_run_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::RunStarted => {
            state.started_at = Some(event.ts.clone());
            state.status = RunStatus::Running;
            if let Some(ref payload) = event.payload {
                if let Some(name) = payload.get("pipeline_name").and_then(|v| v.as_str()) {
                    state.pipeline_name = name.to_string();
                }
                if let Some(run_name) = payload.get("name").and_then(|v| v.as_str()) {
                    if !run_name.is_empty() {
                        state.name = Some(run_name.to_string());
                    }
                }
                if let Some(input) = payload.get("input").and_then(|v| v.as_str()) {
                    state.input = Some(input.to_string());
                }
                if let Some(edges) = payload.get("edges") {
                    if let Ok(parsed) = serde_json::from_value::<Vec<EdgeInfo>>(edges.clone()) {
                        state.edges = parsed;
                    }
                }
                if let Some(node_defs) = payload.get("node_defs") {
                    if let Ok(parsed) =
                        serde_json::from_value::<Vec<NodeDefInfo>>(node_defs.clone())
                    {
                        state.node_defs = parsed;
                    }
                }
                if let Some(tr) = payload.get("target_repo").and_then(|v| v.as_str()) {
                    state.target_repo = Some(tr.to_string());
                }
                if let Some(sb) = payload.get("source_branch").and_then(|v| v.as_str()) {
                    state.source_branch = Some(sb.to_string());
                }
                if let Some(tb) = payload.get("triggered_by").and_then(|v| v.as_str()) {
                    state.triggered_by = Some(tb.to_string());
                }

                let input_images = payload
                    .get("image_filenames")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();

                state.start_node = Some(StartNodeInfo {
                    input_path: "_input/output.md".to_string(),
                    started_at: event.ts.clone(),
                    target_node_ids: entry_node_ids(&state.edges, &state.node_defs),
                    input_images,
                });

                if let Some(end_def) = state.node_defs.iter().find(|n| n.node_type == "end") {
                    state.end_node = Some(EndNodeInfo {
                        id: end_def.id.clone(),
                        ports: end_def
                            .inputs
                            .iter()
                            .map(|port| EndPortStatus {
                                port_name: port.name.clone(),
                                status: "pending".to_string(),
                                reason: None,
                                fired_at: None,
                            })
                            .collect(),
                    });
                }
            }
        }
        EventKind::RunCompleted => {
            state.status = RunStatus::Completed;
            state.completed_at = Some(event.ts.clone());
            if let Some(ref mut end_node) = state.end_node {
                for port in &mut end_node.ports {
                    if port.status == "pending" {
                        port.status = "received".to_string();
                        port.fired_at = Some(event.ts.clone());
                    }
                }
            }
        }
        EventKind::RunFailed => {
            state.status = RunStatus::Failed;
            state.completed_at = Some(event.ts.clone());
        }
        EventKind::RunSkipped => {
            // Graceful no-op (#245): terminal, like RunFailed/RunCompleted.
            // The run reached no `end` node (the selector short-circuited),
            // so end-node ports stay pending — only the run status reflects
            // "fired but nothing to do".
            state.status = RunStatus::Skipped;
            state.completed_at = Some(event.ts.clone());
        }
        EventKind::RunHalted => {
            state.status = RunStatus::Halted;
            state.completed_at = Some(event.ts.clone());
            if let Some(ref mut end_node) = state.end_node {
                let reason = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("message"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                for port in &mut end_node.ports {
                    port.status = "received".to_string();
                    port.reason = reason.clone();
                    port.fired_at = Some(event.ts.clone());
                }
            }
        }
        EventKind::RunPaused => {
            if state.status == RunStatus::Running || state.status == RunStatus::AwaitingUser {
                state.status = RunStatus::Paused;
            }
        }
        EventKind::RunResumed => {
            if state.status == RunStatus::Paused {
                state.status = RunStatus::Running;
            }
        }
        EventKind::RunRenamed => {
            if let Some(ref payload) = event.payload {
                if let Some(new_name) = payload.get("name").and_then(|v| v.as_str()) {
                    if new_name.is_empty() {
                        state.name = None;
                    } else {
                        state.name = Some(new_name.to_string());
                    }
                }
            }
        }
        EventKind::RunArchived => {
            state.status = RunStatus::Archived;
            state.start_node = None;
            state.end_node = None;
        }
        _ => {}
    }
}

/// Node-transition events: the per-iteration lifecycle (waiting -> started ->
/// completed/failed/...), plus stop/stale/invalidate and the frontmatter-retry
/// counter. Node-level status derives from the LATEST iteration (see the
/// `NodeFailed` #196/#212 guard).
fn apply_node_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::NodeWaiting => {
            // Throttled by the session cap: the node is ready but holds no
            // session yet. Mark it `Waiting`; a later `NodeStarted` promotes
            // it to `Running`. No iteration row is opened — the node has not
            // started executing.
            if let Some(ref node_id) = event.node_id {
                let iter = event.iter.unwrap_or(1);
                let node = state
                    .nodes
                    .entry(node_id.clone())
                    .or_insert_with(|| NodeState {
                        node_id: node_id.clone(),
                        status: NodeStatus::Waiting,
                        iter,
                        started_at: None,
                        completed_at: None,
                        failure_reason: None,
                        iterations: Vec::new(),
                        frontmatter_retries: 0,
                        frontmatter_violations: Vec::new(),
                    });
                node.status = NodeStatus::Waiting;
                node.iter = iter;
            }
        }
        EventKind::NodeStarted => {
            if let Some(ref node_id) = event.node_id {
                // Raw count of node sessions spawned (#100). Incremented per
                // `NodeStarted` (not per distinct `(node, iter)`), inside the
                // node-id guard so only real node spawns count — the manager
                // emits no `NodeStarted`.
                state.sessions_spawned += 1;
                let iter = event.iter.unwrap_or(1);
                let iteration = IterationInfo {
                    iter,
                    status: NodeStatus::Running,
                    started_at: Some(event.ts.clone()),
                    completed_at: None,
                };
                let node = state
                    .nodes
                    .entry(node_id.clone())
                    .or_insert_with(|| NodeState {
                        node_id: node_id.clone(),
                        status: NodeStatus::Running,
                        iter,
                        started_at: Some(event.ts.clone()),
                        completed_at: None,
                        failure_reason: None,
                        iterations: Vec::new(),
                        frontmatter_retries: 0,
                        frontmatter_violations: Vec::new(),
                    });
                node.status = NodeStatus::Running;
                node.iter = iter;
                node.started_at = Some(event.ts.clone());
                node.completed_at = None;
                node.failure_reason = None;
                upsert_iteration(&mut node.iterations, iteration);
            }
        }
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.status = NodeStatus::Completed;
                    node.completed_at = Some(event.ts.clone());
                    let iter = event.iter.unwrap_or(node.iter);
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = NodeStatus::Completed;
                        it.completed_at = Some(event.ts.clone());
                    }
                }
            }
        }
        EventKind::NodeAwaitingUser => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.status = NodeStatus::AwaitingUser;
                    let iter = event.iter.unwrap_or(node.iter);
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = NodeStatus::AwaitingUser;
                    }
                }
            }
        }
        EventKind::NodeFailed => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    let iter = event.iter.unwrap_or(node.iter);
                    // Node-level status derives from the LATEST iteration:
                    // failing an older iter (e.g. kill_node on a stale
                    // iter, #196 via #212) must not mislabel a node whose
                    // newer iteration is still live.
                    if iter >= node.iter {
                        node.status = NodeStatus::Failed;
                        node.completed_at = Some(event.ts.clone());
                        if let Some(ref payload) = event.payload {
                            node.failure_reason = payload
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            if let Some(arr) = payload.get("violations").and_then(|v| v.as_array())
                            {
                                node.frontmatter_violations = arr.clone();
                            }
                        }
                    }
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = NodeStatus::Failed;
                        it.completed_at = Some(event.ts.clone());
                    }
                }
            }
        }
        EventKind::NodeStopped => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.status = NodeStatus::Stopped;
                    node.completed_at = Some(event.ts.clone());
                    if let Some(ref payload) = event.payload {
                        node.failure_reason = payload
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                    let iter = event.iter.unwrap_or(node.iter);
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = NodeStatus::Stopped;
                        it.completed_at = Some(event.ts.clone());
                    }
                }
            }
        }
        EventKind::NodeStale => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.status = NodeStatus::Stale;
                    let iter = event.iter.unwrap_or(node.iter);
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = NodeStatus::Stale;
                    }
                }
            }
        }
        EventKind::NodeInvalidated => {
            if let Some(ref node_id) = event.node_id {
                state.nodes.remove(node_id);
            }
        }
        EventKind::FrontmatterRetryPending => {
            if let Some(ref node_id) = event.node_id {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.frontmatter_retries += 1;
                }
            }
        }
        _ => {}
    }
}

/// `SwitchRouted`: a switch node both records its chosen branch in
/// `switch_states` AND writes a synthetic `Completed` node entry (the switch has
/// no NodeRun session of its own), so it is kept as its own concern rather than
/// folded into the node applier. The outer dispatch guarantees the kind, so no
/// inner match is needed.
fn apply_switch_event(state: &mut RunState, event: &Event) {
    if let Some(ref payload) = event.payload {
        if let Some(node_id) = payload.get("node_id").and_then(|v| v.as_str()) {
            let chosen_branch = payload
                .get("chosen_branch")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();

            state.switch_states.insert(
                node_id.to_string(),
                SwitchState {
                    switch_node_id: node_id.to_string(),
                    chosen_branch: chosen_branch.clone(),
                    evaluated_at: event.ts.clone(),
                },
            );

            let iter = event.iter.unwrap_or(1);
            let node = state
                .nodes
                .entry(node_id.to_string())
                .or_insert_with(|| NodeState {
                    node_id: node_id.to_string(),
                    status: NodeStatus::Completed,
                    iter,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                    failure_reason: None,
                    iterations: Vec::new(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                });
            node.status = NodeStatus::Completed;
            node.completed_at = Some(event.ts.clone());
            node.iter = iter;
            upsert_iteration(
                &mut node.iterations,
                IterationInfo {
                    iter,
                    status: NodeStatus::Completed,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                },
            );
        }
    }
}

/// Bounded loop-region lap accounting: track the current/max iteration, the
/// break flag, and the done flag, keyed by `loop_node_id`. `LoopMaxReached` is
/// purely informational.
fn apply_loop_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::LoopIterStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                    let max_iter = payload
                        .get("max_iter")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(5);
                    let ls = state
                        .loop_states
                        .entry(loop_node_id.to_string())
                        .or_insert_with(|| LoopState {
                            loop_node_id: loop_node_id.to_string(),
                            current_iter: 1,
                            max_iter,
                            break_received: false,
                            done: false,
                        });
                    ls.current_iter = iter;
                    ls.max_iter = max_iter;
                }
            }
        }
        EventKind::LoopBreakReceived => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                        ls.break_received = true;
                    }
                }
            }
        }
        EventKind::LoopMaxReached => {
            // Informational
        }
        EventKind::LoopDone => {
            if let Some(ref payload) = event.payload {
                if let Some(loop_node_id) = payload.get("loop_node_id").and_then(|v| v.as_str()) {
                    if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                        ls.done = true;
                    }
                }
            }
        }
        _ => {}
    }
}

/// ForEach barrier accounting: track total items, the break flag, and the done
/// flag, keyed by `foreach_node_id`. An empty list short-circuits straight to
/// done.
fn apply_foreach_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::ForEachStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    let total_items = payload
                        .get("total_items")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    state
                        .foreach_states
                        .entry(foreach_node_id.to_string())
                        .or_insert_with(|| ForEachState {
                            foreach_node_id: foreach_node_id.to_string(),
                            total_items,
                            break_received: false,
                            done: false,
                        });
                }
            }
        }
        EventKind::ForEachEmpty => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    let fs = state
                        .foreach_states
                        .entry(foreach_node_id.to_string())
                        .or_insert_with(|| ForEachState {
                            foreach_node_id: foreach_node_id.to_string(),
                            total_items: 0,
                            break_received: false,
                            done: false,
                        });
                    fs.done = true;
                }
            }
        }
        EventKind::ForEachBreakReceived => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                        fs.break_received = true;
                    }
                }
            }
        }
        EventKind::ForEachDone => {
            if let Some(ref payload) = event.payload {
                if let Some(foreach_node_id) =
                    payload.get("foreach_node_id").and_then(|v| v.as_str())
                {
                    if let Some(fs) = state.foreach_states.get_mut(foreach_node_id) {
                        fs.done = true;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Merge-resolver lifecycle: the conflict signal is informational; the resolver
/// then runs and either completes or fails, tracked in `merge_resolver`.
fn apply_merge_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::MergeConflictDetected => {
            // Informational — the run either spawns a resolver or fails
        }
        EventKind::MergeResolverStarted => {
            if let Some(ref payload) = event.payload {
                let conflicting_node_id = payload
                    .get("conflicting_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                let session_name = payload
                    .get("session_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                state.merge_resolver = Some(MergeResolverInfo {
                    status: NodeStatus::Running,
                    conflicting_node_id,
                    iter,
                    session_name,
                    started_at: Some(event.ts.clone()),
                    completed_at: None,
                    failure_reason: None,
                });
            }
        }
        EventKind::MergeResolverCompleted => {
            if let Some(ref mut mr) = state.merge_resolver {
                mr.status = NodeStatus::Completed;
                mr.completed_at = Some(event.ts.clone());
            }
        }
        EventKind::MergeResolverFailed => {
            if let Some(ref mut mr) = state.merge_resolver {
                mr.status = NodeStatus::Failed;
                mr.completed_at = Some(event.ts.clone());
                if let Some(ref payload) = event.payload {
                    mr.failure_reason = payload
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
        }
        _ => {}
    }
}

/// Pipeline-file events. Both are passive signals that intentionally make NO
/// change to the projected state — `PipelineLint` is informational, and
/// `PipelineModified` must NEVER un-terminalize a run (#221, see below). Kept as
/// its own applier so the load-bearing #221 rationale lives next to the no-op.
fn apply_pipeline_event(_state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::PipelineLint => {
            // Informational — records lint diagnostics for the pipeline
        }
        EventKind::PipelineModified => {
            // The run-scoped pipeline changed on disk. Node_defs/edges are
            // re-parsed from the file at scheduling time
            // (`spawn_ready_after_event`), which picks up newly-added nodes
            // for a *live* (Running/AwaitingUser) run on the next tick — no
            // status change is needed for that, and none happens here.
            //
            // Terminal-state integrity (#221): a `PipelineModified` is a
            // passive signal. It can be emitted by a stray or foreign file
            // write — even for a node that is not in this run's DAG at all —
            // so it must NEVER un-terminalize a run. A run that reached
            // `RunCompleted` (like one that reached `RunFailed`/`RunHalted`,
            // handled below) stays terminal. Reopening a Completed run here
            // left genuinely-finished runs phantom-`running` forever (there
            // was no reliable re-completion path), held their manager
            // session and worktree, made overlap-`skip` triggers skip every
            // subsequent fire, and let a later `resume_run` re-spawn already
            // satisfied loops (the transition guard sees `Running` instead
            // of the true terminal state). Resuming a finished run to pick
            // up newly-added work is an explicit operation (`resume_run`),
            // not a side effect of the file watcher. No status change.
        }
        _ => {}
    }
}

/// `CommandIssued`: the projection-relevant manager/operator commands. A command
/// dispatcher by nature — `resume_run` re-opens a terminal run and `end_region`
/// closes a loop region — so the whole event is kept in one applier even though
/// it touches both run status and `loop_states`. The outer dispatch guarantees
/// the kind, so no inner match is needed.
fn apply_command_event(state: &mut RunState, event: &Event) {
    if let Some(ref payload) = event.payload {
        let cmd = payload.get("command").and_then(|v| v.as_str());
        if cmd == Some("resume_run")
            && (state.status == RunStatus::Halted || state.status == RunStatus::Failed)
        {
            state.status = RunStatus::Running;
            state.completed_at = None;
        }
        // #199: `end_region` CLOSES the region — the projection
        // marks its loop state done so the scheduler's region
        // engine routes the exit instead of starting a phantom lap.
        // A region still on lap 1 has no loop state yet (the entry
        // appears when the first re-entry fires): create it closed,
        // so an early `end_region` is never lost. `max_iter` is
        // unknown to the projection (it lives in the pipeline) and
        // unused once the region is done.
        if cmd == Some("end_region") {
            if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                state
                    .loop_states
                    .entry(region_id.to_string())
                    .or_insert_with(|| LoopState {
                        loop_node_id: region_id.to_string(),
                        current_iter: 1,
                        max_iter: 0,
                        break_received: false,
                        done: false,
                    })
                    .done = true;
            }
        }
    }
}

/// Post-fold reconciliation, run once after every event has been applied.
///
/// Two passes that cannot be done per-event because they depend on the whole
/// fold being complete: (1) sort each node's iterations by `iter` and reconcile
/// the node's top-level `iter` to the latest (handles out-of-order events), and
/// (2) derive run-level `AwaitingUser` from node states — a `Running` run with
/// any awaiting node is itself awaiting the user.
fn finalize(state: &mut RunState) {
    // Sort iterations by iter number and reconcile top-level iter
    // (handles out-of-order events)
    for node in state.nodes.values_mut() {
        node.iterations.sort_by_key(|i| i.iter);
        if let Some(max_iter) = node.iterations.last() {
            node.iter = max_iter.iter;
        }
    }

    // Derive run-level awaiting_user from node states
    if state.status == RunStatus::Running
        && state
            .nodes
            .values()
            .any(|n| n.status == NodeStatus::AwaitingUser)
    {
        state.status = RunStatus::AwaitingUser;
    }
}

/// Is this Run *stalled* (#180)? A run with no node currently `running` or
/// `waiting` and no active merge resolver, where at least one node has gone
/// `stale`, has nothing left to drive it forward: the stale node's session is
/// wedged (idle with incomplete outputs, per `stale_detector`) so its
/// downstream can never be scheduled, and the scheduler — which reacts to every
/// event — has produced no other active node. The run is therefore stuck with
/// no forward progress, which CONTEXT.md's "never a silent stall" requires us
/// to surface (amber dot) instead of leaving it looking active.
///
/// This is a **display-only** derivation: the run's canonical `status` stays
/// `Running`, so stale detection keeps probing it and the stalled state clears
/// automatically as soon as activity resumes (a `NodeStarted`/`NodeWaiting`
/// flips a node back to active, or the stale node completes). We deliberately
/// avoid a persisted `RunStatus::Stale` variant — the condition is recomputed
/// from the projection on every read.
pub fn is_stalled(run: &RunState) -> bool {
    if run.status != RunStatus::Running {
        return false;
    }

    let has_active_node = run
        .nodes
        .values()
        .any(|n| matches!(n.status, NodeStatus::Running | NodeStatus::Waiting));
    let resolver_active = run
        .merge_resolver
        .as_ref()
        .is_some_and(|mr| mr.status == NodeStatus::Running);
    if has_active_node || resolver_active {
        return false;
    }

    run.nodes.values().any(|n| n.status == NodeStatus::Stale)
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn generate_run_id() -> String {
    let now = chrono::Utc::now();
    let ts = now.format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("{ts}-{short}")
}

/// The folded manager routing applied to one bounded loop region by id
/// (ADR-0011 / #152). The Pipeline Manager can route an exhausted-unrouted
/// region: **bump** it (run `bumped_by` more iterations) or **end** it (fire its
/// completion). Both are issued as `CommandIssued` events; this is their
/// projection onto a single region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionRoute {
    /// Extra iterations the manager added on top of the region's `max_iter`
    /// (sum of every `bump_region` command for this id).
    pub bumped_by: i64,
    /// True once the manager ended the region (any `end_region` command for this
    /// id), so the scheduler stops blocking it "exhausted — unrouted".
    pub ended: bool,
}

/// Folds the manager's loop-region routing commands (ADR-0011 / #152) per region
/// id from the event log: `bump_region` accumulates `additional_iter`,
/// `end_region` flips `ended`. The result drives `resume_run` continuation of an
/// exhausted-unrouted region without restarting the daemon.
pub fn collect_region_routes(events: &[Event]) -> HashMap<String, RegionRoute> {
    let mut routes: HashMap<String, RegionRoute> = HashMap::new();
    for event in events {
        if event.kind != EventKind::CommandIssued {
            continue;
        }
        let Some(ref payload) = event.payload else {
            continue;
        };
        let cmd = payload.get("command").and_then(|v| v.as_str());
        let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) else {
            continue;
        };
        match cmd {
            Some("bump_region") => {
                let additional = payload
                    .get("additional_iter")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                routes.entry(region_id.to_string()).or_default().bumped_by += additional;
            }
            Some("end_region") => {
                routes.entry(region_id.to_string()).or_default().ended = true;
            }
            _ => {}
        }
    }
    routes
}

pub fn collect_cycle_extensions(events: &[Event]) -> HashMap<String, i64> {
    let mut extensions: HashMap<String, i64> = HashMap::new();
    for event in events {
        if event.kind != EventKind::CommandIssued {
            continue;
        }
        if let Some(ref payload) = event.payload {
            let cmd = payload.get("command").and_then(|v| v.as_str());
            if cmd == Some("extend_cycle") {
                if let (Some(node_id), Some(additional)) = (
                    payload.get("node_id").and_then(|v| v.as_str()),
                    payload.get("additional_iter").and_then(|v| v.as_i64()),
                ) {
                    *extensions.entry(node_id.to_string()).or_insert(0) += additional;
                }
            }
        }
    }
    extensions
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload: None,
        }
    }

    fn make_event_with_payload(
        kind: EventKind,
        node_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind,
            node_id: node_id.map(String::from),
            iter: None,
            payload: Some(payload),
        }
    }

    #[test]
    fn projects_empty_events_to_none() {
        assert!(project(&[]).is_none());
    }

    #[test]
    fn projects_full_lifecycle() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe", "input": "do the thing" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.run_id, "run-1");
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.pipeline_name, "test-pipe");
        assert_eq!(state.input.as_deref(), Some("do the thing"));
        assert_eq!(state.nodes.len(), 1);

        let node = &state.nodes["planner"];
        assert_eq!(node.status, NodeStatus::Completed);
        assert_eq!(node.iter, 1);
    }

    #[test]
    fn projects_failed_node() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("worker"),
                serde_json::json!({ "reason": "could not complete" }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Failed);
        assert_eq!(node.failure_reason.as_deref(), Some("could not complete"));
        assert!(node.frontmatter_violations.is_empty());
    }

    #[test]
    fn run_skipped_is_a_distinct_terminal_status() {
        // #245: a graceful no-op completes the selector node and marks the run
        // Skipped — distinct from Completed (did work) and Failed (error).
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("selector"), Some(1)),
            make_event_with_payload(
                EventKind::NodeCompleted,
                Some("selector"),
                serde_json::json!({ "skipped": true, "reason": "no eligible issue" }),
            ),
            make_event_with_payload(
                EventKind::RunSkipped,
                None,
                serde_json::json!({ "reason": "no eligible issue" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Skipped);
        assert!(!state.status.is_live(), "skipped run must not be live");
        assert!(state.completed_at.is_some());
        // The node that skipped is honestly terminal-Completed (it ran and
        // reached a decision); only the run reflects "nothing to do".
        assert_eq!(state.nodes["selector"].status, NodeStatus::Completed);
        assert!(
            !is_stalled(&state),
            "a terminal Skipped run is never stalled"
        );
    }

    #[test]
    fn node_failed_on_older_iter_does_not_mislabel_a_live_node() {
        // #196 (via #212): kill_node on iter N while iter N+1 is running must
        // not flip the node to failed — node-level status derives from the
        // latest iteration.
        let mut kill = make_event_with_payload(
            EventKind::NodeFailed,
            Some("worker"),
            serde_json::json!({ "reason": "killed via kill_node command", "source": "kill_node" }),
        );
        kill.iter = Some(1);
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(2)),
            kill,
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Running, "iter 2 is still live");
        assert_eq!(node.iter, 2);
        assert!(node.failure_reason.is_none());
        let it1 = node.iterations.iter().find(|i| i.iter == 1).unwrap();
        assert_eq!(it1.status, NodeStatus::Failed);
        let it2 = node.iterations.iter().find(|i| i.iter == 2).unwrap();
        assert_eq!(it2.status, NodeStatus::Running);
    }

    #[test]
    fn projects_frontmatter_violations_on_failed_node() {
        let violations = serde_json::json!([
            { "port": "review", "field": "verdict", "reason": "value 'MAYBE' not in allowed" },
            { "port": "review", "field": "score", "reason": "expected int" },
        ]);
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("reviewer"),
                serde_json::json!({
                    "reason": "output validation failed",
                    "violations": violations,
                }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["reviewer"];
        assert_eq!(node.status, NodeStatus::Failed);
        assert_eq!(
            node.failure_reason.as_deref(),
            Some("output validation failed")
        );
        assert_eq!(node.frontmatter_violations.len(), 2);
        assert_eq!(node.frontmatter_violations[0]["field"], "verdict");
        assert_eq!(node.frontmatter_violations[1]["field"], "score");
    }

    #[test]
    fn projects_running_state_mid_execution() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "wip" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.nodes["planner"].status, NodeStatus::Running);
    }

    #[test]
    fn sessions_spawned_counts_raw_node_started_not_distinct_iters() {
        // #100: `sessions_spawned` is the RAW count of `NodeStarted` events, so
        // a legal re-spawn at the SAME (node, iter) — restart/recovery — counts
        // again. A distinct-(node,iter) count would undercount real sessions.
        let mut second_a = make_event(EventKind::NodeStarted, Some("a"), Some(1));
        second_a.ts = "2026-01-01T00:05:00.000Z".into();
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("a"), Some(1)),
            make_event(EventKind::NodeStarted, Some("b"), Some(1)),
            second_a, // same (a, 1) again — restart/recovery
        ];

        let state = project(&events).unwrap();
        // 3 raw NodeStarted events, even though only 2 distinct (node, iter).
        assert_eq!(state.sessions_spawned, 3);

        // Sanity: the projection still dedups iterations by (node, iter), so the
        // raw counter must be >= the distinct-iteration total it would yield.
        let distinct_iters: usize = state.nodes.values().map(|n| n.iterations.len()).sum();
        assert_eq!(distinct_iters, 2);
        assert!(state.sessions_spawned as usize >= distinct_iters);
    }

    #[test]
    fn sessions_spawned_ignores_manager_and_non_started_events() {
        // The manager emits no `NodeStarted` (it spawns outside the event-log
        // node path), and a `NodeWaiting` (throttled, no session) must not count.
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeWaiting, Some("a"), Some(1)),
            make_event(EventKind::NodeStarted, Some("a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("a"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.sessions_spawned, 1);
    }

    #[test]
    fn projects_throttled_node_as_waiting_then_running() {
        // A node throttled by the cap enters `waiting`; once a slot frees it is
        // spawned and `node_started` transitions it to `running`.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "capped" }),
            ),
            make_event(EventKind::NodeWaiting, Some("worker"), Some(1)),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.nodes["worker"].status, NodeStatus::Waiting);
        // A run with only a waiting node is still considered Running overall.
        assert_eq!(state.status, RunStatus::Running);

        let mut events = events;
        events.push(make_event(EventKind::NodeStarted, Some("worker"), Some(1)));
        let state = project(&events).unwrap();
        assert_eq!(state.nodes["worker"].status, NodeStatus::Running);
    }

    #[test]
    fn projects_interactive_node_awaiting_user() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "interactive-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert_eq!(state.nodes["griller"].status, NodeStatus::AwaitingUser);
    }

    #[test]
    fn mark_node_done_completes_awaiting_node() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "interactive-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("griller"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.nodes["griller"].status, NodeStatus::Completed);
    }

    #[test]
    fn projects_archived_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "archival-test", "input": "test input" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Archived);
        assert_eq!(state.pipeline_name, "archival-test");
        assert_eq!(state.nodes.len(), 1);
        assert_eq!(state.nodes["worker"].status, NodeStatus::Completed);
    }

    #[test]
    fn projects_halted_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "halt-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::RunHalted,
                None,
                serde_json::json!({ "message": "Blocked after 3 iterations" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Halted);
        assert!(state.completed_at.is_some());
    }

    #[test]
    fn projects_merge_conflict_halts_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "merge-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-1"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-1"), Some(1)),
            Event {
                id: None,
                run_id: "run-1".into(),
                ts: "2026-01-01T00:00:00.000Z".into(),
                kind: EventKind::MergeConflictDetected,
                node_id: Some("impl-1".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({
                    "reason": "conflict merging impl-1 into pipeline branch"
                })),
            },
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
    }

    #[test]
    fn pipeline_modified_after_completed_stays_completed() {
        // #221: a `PipelineModified` is a passive signal (it can be a stray or
        // foreign file write) and must NEVER un-terminalize a genuinely-
        // completed run. Reopening it left runs phantom-`running` forever with
        // no reliable re-completion path. A terminal run stays terminal — the
        // same way Failed/Halted are not reopened.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe", "input": "do the thing" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Completed,
            "PipelineModified after RunCompleted must NOT reopen the run (#221)"
        );
        assert!(
            state.completed_at.is_some(),
            "completed_at must be preserved across a post-completion PipelineModified"
        );
    }

    #[test]
    fn pipeline_modified_storm_after_completed_stays_completed() {
        // The incident (#221) saw a foreign prompt write followed by more
        // pipeline churn. No quantity of passive PipelineModified events may
        // flip a terminal run back to running.
        let mut events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];
        for _ in 0..5 {
            events.push(make_event(EventKind::PipelineModified, None, None));
        }

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert!(state.completed_at.is_some());
    }

    #[test]
    fn pipeline_modified_after_halted_stays_halted() {
        // Parity with the Failed case: a halted run is terminal and is not
        // reopened by a passive pipeline modification either.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::RunHalted,
                None,
                serde_json::json!({ "message": "exhausted — unrouted" }),
            ),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Halted,
            "PipelineModified should not reopen a Halted run"
        );
    }

    #[test]
    fn pipeline_modified_during_running_stays_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test-pipe" }),
            ),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn pipeline_modified_after_failed_stays_failed() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeFailed, None, None),
            make_event(EventKind::RunFailed, None, None),
            make_event(EventKind::PipelineModified, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Failed,
            "PipelineModified should not reopen a Failed run"
        );
    }

    #[test]
    fn run_id_format() {
        let id = generate_run_id();
        // Format: YYYYMMDD-HHMMSS-<7char>
        assert!(id.len() >= 22, "run-id too short: {id}");
        assert!(id.contains('-'));
    }

    // --- start_node projection (issue #30, updated for #39) ---

    fn start_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "start", "node_type": "start", "inputs": [], "outputs": [{"name": "user_prompt", "side": "right"}] })
    }

    fn end_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "end", "node_type": "end", "inputs": [{"name": "result", "side": "left"}], "outputs": [] })
    }

    fn node_def(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "node_type": "doc-only",
            "inputs": [{"name": "task", "side": "left"}],
            "outputs": [{"name": "out", "side": "right"}]
        })
    }

    fn edge_info(src: &str, tgt: &str) -> serde_json::Value {
        serde_json::json!({
            "source_node": src, "source_port": "out",
            "target_node": tgt, "target_port": "task"
        })
    }

    fn edge_info_conditional(src: &str, tgt: &str, when: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "source_node": src, "source_port": "out",
            "target_node": tgt, "target_port": "task",
            "when_clause": when
        })
    }

    #[test]
    fn start_node_single_entry() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "linear",
                "input": "hello world",
                "node_defs": [start_node_def(), end_node_def(), node_def("planner"), node_def("implementer")],
                "edges": [
                    edge_info("start", "planner"),
                    edge_info("planner", "implementer"),
                    edge_info("implementer", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.input_path, "_input/output.md");
        assert_eq!(start.started_at, "2026-01-01T00:00:00.000Z");
        assert_eq!(start.target_node_ids, vec!["planner"]);
        assert!(
            start.input_images.is_empty(),
            "a run with no uploaded images carries no input_images"
        );
    }

    #[test]
    fn start_node_carries_uploaded_input_images() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "linear",
                "input": "look at these",
                "image_filenames": ["ui-bug.png", "trace.png"],
                "node_defs": [start_node_def(), end_node_def(), node_def("planner")],
                "edges": [
                    edge_info("start", "planner"),
                    edge_info("planner", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.input_images, vec!["ui-bug.png", "trace.png"]);
    }

    #[test]
    fn start_node_multiple_entry_nodes_fan_out() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "fan-out",
                "input": "build two things",
                "node_defs": [start_node_def(), end_node_def(), node_def("impl-a"), node_def("impl-b"), node_def("merger")],
                "edges": [
                    edge_info("start", "impl-a"),
                    edge_info("start", "impl-b"),
                    edge_info("impl-a", "merger"),
                    edge_info("impl-b", "merger"),
                    edge_info("merger", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        let mut targets = start.target_node_ids.clone();
        targets.sort();
        assert_eq!(targets, vec!["impl-a", "impl-b"]);
    }

    #[test]
    fn start_node_conditional_back_edge_not_counted() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "cycle",
                "input": "iterate",
                "node_defs": [start_node_def(), end_node_def(), node_def("implementer"), node_def("reviewer")],
                "edges": [
                    edge_info("start", "implementer"),
                    edge_info("implementer", "reviewer"),
                    edge_info_conditional("reviewer", "implementer", serde_json::json!({"iter": {"lt": 3}})),
                    edge_info("reviewer", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.target_node_ids, vec!["implementer"]);
    }

    #[test]
    fn start_node_null_on_archived_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "archived-test",
                    "input": "test input",
                    "node_defs": [start_node_def(), end_node_def(), node_def("only")],
                    "edges": [edge_info("start", "only"), edge_info("only", "end")],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("only"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("only"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Archived);
        assert!(state.start_node.is_none());
    }

    #[test]
    fn start_node_all_nodes_are_entry_when_no_inter_edges() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "isolated",
                "input": "go",
                "node_defs": [start_node_def(), end_node_def(), node_def("a"), node_def("b")],
                "edges": [edge_info("start", "a"), edge_info("start", "b")],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        let mut targets = start.target_node_ids.clone();
        targets.sort();
        assert_eq!(targets, vec!["a", "b"]);
    }

    #[test]
    fn start_node_end_edges_dont_block_entry() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "with-end",
                "input": "test",
                "node_defs": [start_node_def(), end_node_def(), node_def("reviewer")],
                "edges": [
                    edge_info("start", "reviewer"),
                    {
                        "source_node": "reviewer", "source_port": "review",
                        "target_node": "end", "target_port": "result",
                        "halt_message": "Blocked",
                        "when_clause": {"iter": {"gte": 3}}
                    },
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let start = state.start_node.as_ref().unwrap();
        assert_eq!(start.target_node_ids, vec!["reviewer"]);
    }

    // --- Multi-iteration projection tests (issue #29) ---

    fn make_event_ts(kind: EventKind, node_id: Option<&str>, iter: Option<i64>, ts: &str) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: ts.into(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload: None,
        }
    }

    #[test]
    fn single_iter_node_has_one_iteration_entry() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "test" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("planner"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("planner"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["planner"];
        assert_eq!(node.iterations.len(), 1);
        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(
            node.iterations[0].started_at.as_deref(),
            Some("2026-01-01T00:01:00.000Z")
        );
        assert_eq!(
            node.iterations[0].completed_at.as_deref(),
            Some("2026-01-01T00:02:00.000Z")
        );
    }

    #[test]
    fn multi_iter_cycle_produces_ordered_iterations() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "cycle-test" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("reviewer"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("reviewer"),
                Some(2),
                "2026-01-01T00:04:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("reviewer"),
                Some(3),
                "2026-01-01T00:05:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["reviewer"];

        assert_eq!(node.iter, 3, "top-level iter should be the latest");
        assert_eq!(
            node.status,
            NodeStatus::Running,
            "current status is running"
        );
        assert_eq!(node.iterations.len(), 3);

        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);

        assert_eq!(node.iterations[1].iter, 2);
        assert_eq!(node.iterations[1].status, NodeStatus::Completed);

        assert_eq!(node.iterations[2].iter, 3);
        assert_eq!(node.iterations[2].status, NodeStatus::Running);
        assert!(node.iterations[2].completed_at.is_none());
    }

    #[test]
    fn multi_iter_with_failed_iteration() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fail-iter" }),
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("impl"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("impl"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("impl"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            {
                let mut e = make_event_ts(
                    EventKind::NodeFailed,
                    Some("impl"),
                    Some(2),
                    "2026-01-01T00:04:00.000Z",
                );
                e.payload = Some(serde_json::json!({ "reason": "test failure" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["impl"];

        assert_eq!(node.iterations.len(), 2);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(node.iterations[1].status, NodeStatus::Failed);
    }

    #[test]
    fn out_of_order_node_started_events_still_project_correctly() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "ooo" }),
            ),
            // iter 2 event arrives before iter 1 completes (out-of-order)
            make_event_ts(
                EventKind::NodeStarted,
                Some("worker"),
                Some(2),
                "2026-01-01T00:03:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                "2026-01-01T00:01:00.000Z",
            ),
            make_event_ts(
                EventKind::NodeCompleted,
                Some("worker"),
                Some(1),
                "2026-01-01T00:02:00.000Z",
            ),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];

        // iterations should be sorted by iter number
        assert_eq!(node.iterations.len(), 2);
        assert_eq!(node.iterations[0].iter, 1);
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
        assert_eq!(node.iterations[1].iter, 2);
        assert_eq!(node.iterations[1].status, NodeStatus::Running);

        // top-level iter reflects the highest
        assert_eq!(node.iter, 2);
    }

    #[test]
    fn existing_tests_still_get_empty_iterations_for_single_iter() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "compat" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        // Even single-iter nodes have exactly 1 iteration entry
        assert_eq!(node.iterations.len(), 1);
    }

    #[test]
    fn resume_run_transitions_halted_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunHalted, None, None),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn resume_run_transitions_failed_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunFailed, None, None),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn resume_run_noop_on_already_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn collect_cycle_extensions_accumulates() {
        let events = vec![
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "review",
                    "additional_iter": 2
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "review",
                    "additional_iter": 3
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "other",
                    "additional_iter": 1
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let ext = collect_cycle_extensions(&events);
        assert_eq!(ext["review"], 5);
        assert_eq!(ext["other"], 1);
    }

    #[test]
    fn command_issued_unknown_command_is_noop() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({ "command": "something_unknown" })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    // --- End node projection tests (issue #39) ---

    #[test]
    fn end_node_pending_while_running() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "with-end",
                "input": "go",
                "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                "edges": [
                    edge_info("start", "worker"),
                    edge_info("worker", "end"),
                ],
            }),
        )];

        let state = project(&events).unwrap();
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.id, "end");
        assert_eq!(end.ports.len(), 1);
        assert_eq!(end.ports[0].port_name, "result");
        assert_eq!(end.ports[0].status, "pending");
        assert!(end.ports[0].reason.is_none());
        assert!(end.ports[0].fired_at.is_none());
    }

    #[test]
    fn end_node_received_on_run_completed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "complete-test",
                    "input": "go",
                    "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                    "edges": [
                        edge_info("start", "worker"),
                        edge_info("worker", "end"),
                    ],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.ports[0].status, "received");
        assert!(end.ports[0].reason.is_none());
        assert!(end.ports[0].fired_at.is_some());
    }

    #[test]
    fn end_node_received_with_reason_on_halt() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "halt-end-test",
                    "input": "iterate",
                    "node_defs": [start_node_def(), end_node_def(), node_def("reviewer")],
                    "edges": [
                        edge_info("start", "reviewer"),
                        {
                            "source_node": "reviewer", "source_port": "review",
                            "target_node": "end", "target_port": "result",
                            "halt_message": "Blocked after 3 iterations",
                            "when_clause": {"iter": {"gte": 3}}
                        },
                    ],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::RunHalted,
                None,
                serde_json::json!({ "message": "Blocked after 3 iterations" }),
            ),
        ];

        let state = project(&events).unwrap();
        let end = state.end_node.as_ref().expect("end_node should be present");
        assert_eq!(end.ports[0].status, "received");
        assert_eq!(
            end.ports[0].reason.as_deref(),
            Some("Blocked after 3 iterations")
        );
        assert!(end.ports[0].fired_at.is_some());
    }

    #[test]
    fn end_node_cleared_on_archived() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "archive-end-test",
                    "input": "go",
                    "node_defs": [start_node_def(), end_node_def(), node_def("worker")],
                    "edges": [
                        edge_info("start", "worker"),
                        edge_info("worker", "end"),
                    ],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunArchived, None, None),
        ];

        let state = project(&events).unwrap();
        assert!(state.end_node.is_none());
    }

    // --- Merge Resolver projection tests (issue #8) ---

    #[test]
    fn merge_resolver_full_lifecycle_conflict_to_completion() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fan-in" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeStarted, Some("impl-b"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-b"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-b"),
                serde_json::json!({
                    "reason": "conflict merging impl-b into pipeline branch"
                }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({
                    "conflicting_node_id": "impl-b",
                    "iter": 1,
                    "session_name": "pdo-run-1-__merge_resolver__-iter-1"
                }),
            ),
            make_event(EventKind::MergeResolverCompleted, None, None),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.nodes["impl-a"].status, NodeStatus::Completed);
        assert_eq!(state.nodes["impl-b"].status, NodeStatus::Completed);

        let mr = state.merge_resolver.as_ref().unwrap();
        assert_eq!(mr.status, NodeStatus::Completed);
        assert_eq!(mr.conflicting_node_id, "impl-b");
        assert_eq!(mr.iter, 1);
        assert_eq!(
            mr.session_name.as_deref(),
            Some("pdo-run-1-__merge_resolver__-iter-1")
        );
        assert!(mr.completed_at.is_some());
        assert!(mr.failure_reason.is_none());
    }

    #[test]
    fn merge_resolver_failure_preserves_info_on_run_failed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fan-in" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-a"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-a"),
                serde_json::json!({ "reason": "conflict" }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({
                    "conflicting_node_id": "impl-a",
                    "iter": 1,
                    "session_name": "resolver-session"
                }),
            ),
            make_event_with_payload(
                EventKind::MergeResolverFailed,
                None,
                serde_json::json!({
                    "reason": "conflict markers remain"
                }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);

        let mr = state.merge_resolver.as_ref().unwrap();
        assert_eq!(mr.status, NodeStatus::Failed);
        assert_eq!(mr.conflicting_node_id, "impl-a");
        assert_eq!(mr.session_name.as_deref(), Some("resolver-session"));
        assert_eq!(
            mr.failure_reason.as_deref(),
            Some("conflict markers remain")
        );
    }

    #[test]
    fn merge_conflict_without_resolver_has_no_merge_resolver_info() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "no-resolver" }),
            ),
            make_event(EventKind::NodeStarted, Some("impl-1"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("impl-1"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl-1"),
                serde_json::json!({ "reason": "conflict" }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        assert!(
            state.merge_resolver.is_none(),
            "no resolver should be present when merge is handled by Merge node"
        );
    }

    // --- ForEach integration tests ---

    #[test]
    fn foreach_full_lifecycle_3_items() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-test", "input": "go" }),
            ),
            make_event(EventKind::NodeStarted, Some("upstream"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("upstream"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachStarted,
                None,
                serde_json::json!({ "foreach_node_id": "fe1", "total_items": 3 }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(2)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(3)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(2)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(3)),
            make_event_with_payload(
                EventKind::ForEachDone,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);

        let fe_state = &state.foreach_states["fe1"];
        assert_eq!(fe_state.total_items, 3);
        assert!(fe_state.done);
        assert!(!fe_state.break_received);

        let worker = &state.nodes["worker"];
        assert_eq!(worker.iter, 3);
        assert_eq!(worker.iterations.len(), 3);
        for it in &worker.iterations {
            assert_eq!(it.status, NodeStatus::Completed);
        }
    }

    #[test]
    fn foreach_empty_list_completes_immediately() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-empty" }),
            ),
            make_event(EventKind::NodeStarted, Some("upstream"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("upstream"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachEmpty,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event_with_payload(
                EventKind::ForEachDone,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);

        let fe_state = &state.foreach_states["fe1"];
        assert!(fe_state.done);
        assert_eq!(fe_state.total_items, 0);
    }

    #[test]
    fn foreach_break_received_sets_flag() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "foreach-break" }),
            ),
            make_event_with_payload(
                EventKind::ForEachStarted,
                None,
                serde_json::json!({ "foreach_node_id": "fe1", "total_items": 3 }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::ForEachBreakReceived,
                None,
                serde_json::json!({ "foreach_node_id": "fe1" }),
            ),
        ];

        let state = project(&events).unwrap();
        let fe_state = &state.foreach_states["fe1"];
        assert!(fe_state.break_received);
        assert!(!fe_state.done);
    }

    // --- Run display labels (issue #115) ---

    #[test]
    fn run_started_with_name_sets_display_name() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff",
                "name": "My Feature Run"
            }),
        )];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("My Feature Run"));
        assert_eq!(state.pipeline_name, "test-pipe");
    }

    #[test]
    fn run_started_without_name_has_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff"
            }),
        )];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    #[test]
    fn run_started_with_empty_name_has_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "do stuff",
                "name": ""
            }),
        )];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    #[test]
    fn run_renamed_updates_display_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "input": "do stuff"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "Better Name" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("Better Name"));
    }

    #[test]
    fn run_renamed_overwrites_previous_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "name": "First Name"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "Second Name" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.name.as_deref(), Some("Second Name"));
    }

    #[test]
    fn run_renamed_to_empty_clears_name() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "test-pipe",
                    "name": "Had a Name"
                }),
            ),
            make_event_with_payload(
                EventKind::RunRenamed,
                None,
                serde_json::json!({ "name": "" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert!(state.name.is_none());
    }

    // --- New event kinds and statuses (issue #112) ---

    #[test]
    fn node_stopped_sets_stopped_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stop-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            {
                let mut e = make_event(EventKind::NodeStopped, Some("worker"), Some(1));
                e.payload = Some(serde_json::json!({ "reason": "user killed it" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Stopped);
        assert_eq!(node.failure_reason.as_deref(), Some("user killed it"));
        assert!(node.completed_at.is_some());
        assert_eq!(node.iterations[0].status, NodeStatus::Stopped);
    }

    #[test]
    fn node_stopped_does_not_fail_the_run() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stop-run-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            {
                let mut e = make_event(EventKind::NodeStopped, Some("worker"), Some(1));
                e.payload = Some(serde_json::json!({ "reason": "deliberate stop" }));
                e
            },
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Running,
            "NodeStopped must NOT transition the run to failed"
        );
    }

    #[test]
    fn node_auto_completed_sets_completed_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "auto-complete-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeAutoCompleted, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Completed);
        assert!(node.completed_at.is_some());
        assert_eq!(node.iterations[0].status, NodeStatus::Completed);
    }

    #[test]
    fn node_stale_sets_stale_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "stale-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Stale);
        assert!(node.completed_at.is_none(), "stale nodes are not completed");
        assert_eq!(node.iterations[0].status, NodeStatus::Stale);
    }

    #[test]
    fn run_paused_sets_paused_status() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "pause-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Paused);
    }

    #[test]
    fn run_resumed_returns_to_running() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "resume-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
            make_event(EventKind::RunResumed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn run_paused_from_awaiting_user() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "pause-await" }),
            ),
            make_event(EventKind::NodeStarted, Some("griller"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("griller"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Paused);
    }

    #[test]
    fn run_paused_noop_when_already_completed() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunCompleted, None, None),
            make_event(EventKind::RunPaused, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Completed,
            "RunPaused should not affect a completed run"
        );
    }

    #[test]
    fn run_resumed_noop_when_not_paused() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::RunResumed, None, None),
        ];

        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Running,
            "RunResumed on non-paused run is a no-op"
        );
    }

    #[test]
    fn event_kind_serialization_roundtrip() {
        let kinds = vec![
            EventKind::NodeStopped,
            EventKind::NodeAutoCompleted,
            EventKind::NodeStale,
            EventKind::NodeBlockedOnLimit,
            EventKind::RunPaused,
            EventKind::RunResumed,
        ];
        let expected_strings = vec![
            "\"node_stopped\"",
            "\"node_auto_completed\"",
            "\"node_stale\"",
            "\"node_blocked_on_limit\"",
            "\"run_paused\"",
            "\"run_resumed\"",
        ];
        for (kind, expected) in kinds.into_iter().zip(expected_strings) {
            let serialized = serde_json::to_string(&kind).unwrap();
            assert_eq!(serialized, expected);
            let deserialized: EventKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, kind);
        }
    }

    #[test]
    fn node_status_serialization_roundtrip() {
        let statuses = vec![NodeStatus::Stopped, NodeStatus::Stale];
        let expected = vec!["\"stopped\"", "\"stale\""];
        for (status, exp) in statuses.into_iter().zip(expected) {
            let s = serde_json::to_string(&status).unwrap();
            assert_eq!(s, exp);
            let d: NodeStatus = serde_json::from_str(&s).unwrap();
            assert_eq!(d, status);
        }
    }

    // --- SwitchRouted projection (issue #118) ---

    #[test]
    fn switch_routed_creates_synthetic_completed_node() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "switch-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "pass",
                }),
            ),
        ];

        let state = project(&events).unwrap();

        // Switch should have synthetic Completed status
        let sw_node = &state.nodes["sw"];
        assert_eq!(sw_node.status, NodeStatus::Completed);
        assert!(sw_node.started_at.is_some());
        assert!(sw_node.completed_at.is_some());

        // SwitchState should track chosen branch
        let sw_state = &state.switch_states["sw"];
        assert_eq!(sw_state.chosen_branch, "pass");
        assert_eq!(sw_state.switch_node_id, "sw");
    }

    #[test]
    fn switch_routed_updates_on_re_evaluation() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "switch-test" }),
            ),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "default",
                }),
            ),
            make_event_with_payload(
                EventKind::SwitchRouted,
                Some("sw"),
                serde_json::json!({
                    "node_id": "sw",
                    "chosen_branch": "pass",
                }),
            ),
        ];

        let state = project(&events).unwrap();
        let sw_state = &state.switch_states["sw"];
        assert_eq!(
            sw_state.chosen_branch, "pass",
            "re-evaluation should update chosen_branch"
        );
    }

    #[test]
    fn run_status_paused_serialization_roundtrip() {
        let s = serde_json::to_string(&RunStatus::Paused).unwrap();
        assert_eq!(s, "\"paused\"");
        let d: RunStatus = serde_json::from_str(&s).unwrap();
        assert_eq!(d, RunStatus::Paused);
    }

    #[test]
    fn node_invalidated_removes_node_from_state() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "invalidate-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeInvalidated, Some("worker"), None),
        ];

        let state = project(&events).unwrap();
        assert!(
            !state.nodes.contains_key("worker"),
            "NodeInvalidated should remove the node from state"
        );
    }

    #[test]
    fn node_invalidated_allows_re_start() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "retry-test" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeInvalidated, Some("worker"), None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(2)),
        ];

        let state = project(&events).unwrap();
        let node = &state.nodes["worker"];
        assert_eq!(node.status, NodeStatus::Running);
        assert_eq!(node.iter, 2);
        assert_eq!(node.iterations.len(), 1);
    }

    #[test]
    fn node_invalidated_serialization_roundtrip() {
        let kind = EventKind::NodeInvalidated;
        let serialized = serde_json::to_string(&kind).unwrap();
        assert_eq!(serialized, "\"node_invalidated\"");
        let deserialized: EventKind = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, kind);
    }

    // ── Manager loop-region routing (ADR-0011 / #152) ────────────────────────

    #[test]
    fn end_region_marks_the_region_ended() {
        // The manager ends a bounded region by id (fire its completion): the
        // folded route for that id is `ended`, so the scheduler stops blocking
        // it "exhausted — unrouted".
        let events = vec![make_event_with_payload(
            EventKind::CommandIssued,
            None,
            serde_json::json!({ "command": "end_region", "region_id": "review_loop" }),
        )];
        let routes = collect_region_routes(&events);
        let route = routes.get("review_loop").expect("review_loop routed");
        assert!(route.ended, "end_region marks the region ended");
        assert_eq!(route.bumped_by, 0, "end_region adds no extra iterations");
    }

    #[test]
    fn end_region_projects_the_region_loop_state_as_done() {
        // #199: end_region must CLOSE the region, not start a phantom lap. The
        // projection marks the region's loop state done, so the scheduler's
        // region engine routes the exit instead of re-spawning the entry.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "loop-test" }),
            ),
            make_event_with_payload(
                EventKind::LoopIterStarted,
                None,
                serde_json::json!({ "loop_node_id": "review_loop", "iter": 1, "max_iter": 3 }),
            ),
            make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "end_region", "region_id": "review_loop" }),
            ),
        ];
        let state = project(&events).unwrap();
        let ls = state
            .loop_states
            .get("review_loop")
            .expect("region has a loop state");
        assert!(ls.done, "end_region closes the region in the projection");
    }

    #[test]
    fn end_region_during_lap_one_creates_the_loop_state_closed() {
        // A region on lap 1 has no loop state yet (the entry appears when the
        // first re-entry fires). An end_region issued at that point must not
        // be lost: the projection creates the state closed, so the region
        // engine routes the exit instead of starting lap 2.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "loop-test" }),
            ),
            make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "end_region", "region_id": "review_loop" }),
            ),
        ];
        let state = project(&events).unwrap();
        let ls = state
            .loop_states
            .get("review_loop")
            .expect("end_region creates the loop state when missing");
        assert!(ls.done, "the created loop state is closed");
        assert_eq!(ls.current_iter, 1, "the region never went past lap 1");
    }

    #[test]
    fn bump_region_accumulates_additional_iterations() {
        // Two bumps of +2 and +3 on the same region id sum to +5 extra laps; the
        // region is not ended (the manager chose to keep iterating).
        let events = vec![
            make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({
                    "command": "bump_region",
                    "region_id": "review_loop",
                    "additional_iter": 2,
                }),
            ),
            make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({
                    "command": "bump_region",
                    "region_id": "review_loop",
                    "additional_iter": 3,
                }),
            ),
        ];
        let routes = collect_region_routes(&events);
        let route = routes.get("review_loop").expect("review_loop routed");
        assert_eq!(route.bumped_by, 5, "bumps accumulate");
        assert!(!route.ended, "bump does not end the region");
    }

    #[test]
    fn region_routes_are_keyed_per_region_id() {
        // Routing one region leaves a sibling region untouched: routes are keyed
        // by region id, so the manager unsticks exactly the region it named.
        let events = vec![make_event_with_payload(
            EventKind::CommandIssued,
            None,
            serde_json::json!({ "command": "end_region", "region_id": "review_loop" }),
        )];
        let routes = collect_region_routes(&events);
        assert!(routes.contains_key("review_loop"));
        assert!(
            !routes.contains_key("other_loop"),
            "an unrouted sibling region has no route entry"
        );
    }

    // --- RunState / RunStatus / NodeStatus query interface (#237) ---

    fn node_state(
        id: &str,
        status: NodeStatus,
        iter: i64,
        iters: &[(i64, NodeStatus)],
    ) -> NodeState {
        NodeState {
            node_id: id.to_string(),
            status,
            iter,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            iterations: iters
                .iter()
                .map(|(i, s)| IterationInfo {
                    iter: *i,
                    status: s.clone(),
                    started_at: None,
                    completed_at: None,
                })
                .collect(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn run_with(nodes: Vec<NodeState>) -> RunState {
        let mut s = RunState::new("run-1".into(), "test".into());
        for n in nodes {
            s.nodes.insert(n.node_id.clone(), n);
        }
        s
    }

    #[test]
    fn node_status_predicates_diverge_only_on_waiting() {
        use NodeStatus::*;
        let all = [
            Pending,
            Waiting,
            Running,
            AwaitingUser,
            Completed,
            Failed,
            Stopped,
            Stale,
        ];
        for s in &all {
            match s {
                Running | AwaitingUser => {
                    assert!(s.holds_session(), "{s:?} holds a NodeRun session");
                    assert!(s.can_progress(), "{s:?} can drive the run forward");
                }
                Waiting => {
                    assert!(
                        !s.holds_session(),
                        "Waiting holds NO session yet (no admission slot consumed)"
                    );
                    assert!(
                        s.can_progress(),
                        "Waiting CAN progress (it spawns once a slot frees)"
                    );
                }
                Pending | Completed | Failed | Stopped | Stale => {
                    assert!(!s.holds_session(), "{s:?} holds no session");
                    assert!(!s.can_progress(), "{s:?} cannot drive the run forward");
                }
            }
        }
        // The load-bearing fact: the admission set and the stall set differ on
        // exactly one variant — `Waiting`. Collapsing them is the #237 trap.
        assert!(
            !Waiting.holds_session() && Waiting.can_progress(),
            "the admission-vs-stall divergence lives entirely on Waiting"
        );
    }

    #[test]
    fn run_status_is_terminal_is_the_total_complement_of_is_live() {
        use RunStatus::*;
        let all = [
            Running,
            AwaitingUser,
            Completed,
            Failed,
            Skipped,
            Halted,
            Paused,
            Archived,
        ];
        for s in &all {
            assert_eq!(
                s.is_terminal(),
                !s.is_live(),
                "{s:?}: is_terminal must be the exact complement of is_live"
            );
        }
        // Spot-check the variants the partition is easy to get wrong.
        assert!(
            !Paused.is_terminal(),
            "Paused is live (resumable, holds a slot, blocks overlap) — NOT terminal"
        );
        assert!(Skipped.is_terminal(), "Skipped is a terminal no-op (#245)");
        assert!(Archived.is_terminal(), "Archived is terminal");
        assert!(Completed.is_terminal());
        assert!(Failed.is_terminal());
        assert!(Halted.is_terminal());
        assert!(!Running.is_terminal());
        assert!(!AwaitingUser.is_terminal());
    }

    #[test]
    fn latest_completed_iter_quarantines_failed_iterations() {
        // #210: failed iter 1 then completed iter 2 → resolves to iter 2.
        let s = run_with(vec![node_state(
            "a",
            NodeStatus::Completed,
            2,
            &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
        )]);
        assert_eq!(s.latest_completed_iter("a"), Some(2));
    }

    #[test]
    fn latest_completed_iter_picks_the_max_completed_history_iter() {
        let s = run_with(vec![node_state(
            "a",
            NodeStatus::Completed,
            1,
            &[(1, NodeStatus::Completed)],
        )]);
        assert_eq!(s.latest_completed_iter("a"), Some(1));
    }

    #[test]
    fn latest_completed_iter_falls_back_to_head_when_history_is_empty() {
        // Legacy state: head status Completed, no per-iteration history recorded.
        let s = run_with(vec![node_state("a", NodeStatus::Completed, 4, &[])]);
        assert_eq!(s.latest_completed_iter("a"), Some(4));
    }

    #[test]
    fn latest_completed_iter_is_none_when_nothing_completed() {
        let s = run_with(vec![node_state(
            "a",
            NodeStatus::Running,
            1,
            &[(1, NodeStatus::Running)],
        )]);
        assert_eq!(s.latest_completed_iter("a"), None);
    }

    #[test]
    fn latest_completed_iter_is_none_for_an_absent_node() {
        let s = run_with(vec![]);
        assert_eq!(s.latest_completed_iter("ghost"), None);
    }

    #[test]
    fn all_nodes_completed_true_only_when_every_id_is_completed() {
        let s = run_with(vec![
            node_state("a", NodeStatus::Completed, 1, &[]),
            node_state("b", NodeStatus::Completed, 1, &[]),
        ]);
        assert!(s.all_nodes_completed(&["a".into(), "b".into()]));
    }

    #[test]
    fn all_nodes_completed_is_false_on_an_empty_set() {
        // NOT vacuous-true: a run with no expected nodes is not "all done".
        let s = run_with(vec![node_state("a", NodeStatus::Completed, 1, &[])]);
        assert!(!s.all_nodes_completed(&[]));
    }

    #[test]
    fn all_nodes_completed_is_completed_only_never_terminal_tolerant() {
        let s = run_with(vec![
            node_state("a", NodeStatus::Completed, 1, &[]),
            node_state("b", NodeStatus::Failed, 1, &[]),
        ]);
        assert!(
            !s.all_nodes_completed(&["a".into(), "b".into()]),
            "a Failed node is not Completed — completed-only, never terminal-tolerant"
        );
    }

    #[test]
    fn all_nodes_completed_counts_a_missing_node_as_not_done() {
        let s = run_with(vec![node_state("a", NodeStatus::Completed, 1, &[])]);
        assert!(
            !s.all_nodes_completed(&["a".into(), "b".into()]),
            "a never-spawned id (no NodeState) counts as not-done"
        );
    }

    #[test]
    fn all_nodes_completed_does_not_let_an_out_of_set_node_rescue_a_missing_one() {
        // `c` is Completed but is not in the queried slice; it must not mask the
        // absence of `b`.
        let s = run_with(vec![
            node_state("a", NodeStatus::Completed, 1, &[]),
            node_state("c", NodeStatus::Completed, 1, &[]),
        ]);
        assert!(!s.all_nodes_completed(&["a".into(), "b".into()]));
    }

    #[test]
    fn node_status_returns_the_status_for_a_present_node_and_none_otherwise() {
        let s = run_with(vec![node_state("a", NodeStatus::Running, 1, &[])]);
        assert_eq!(s.node_status("a"), Some(&NodeStatus::Running));
        assert_eq!(s.node_status("absent"), None);
    }

    // --- is_stalled: run-level stale derivation (#180) ---

    #[test]
    fn stalled_when_only_node_went_stale() {
        // A node went stale and nothing else is running/waiting: the run has no
        // forward progress, yet its canonical status stays Running.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "wedged" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running, "status stays Running");
        assert_eq!(state.nodes["worker"].status, NodeStatus::Stale);
        assert!(is_stalled(&state), "all-idle with a stale node => stalled");
    }

    #[test]
    fn not_stalled_when_another_node_still_running() {
        // One branch is stale but a sibling is still running: the run is making
        // progress and must NOT be flagged stale.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "fan-out" }),
            ),
            make_event(EventKind::NodeStarted, Some("a"), Some(1)),
            make_event(EventKind::NodeStarted, Some("b"), Some(1)),
            make_event(EventKind::NodeStale, Some("a"), Some(1)),
        ];
        let state = project(&events).unwrap();
        assert!(
            !is_stalled(&state),
            "a still-running sibling means the run is progressing"
        );
    }

    #[test]
    fn not_stalled_when_a_node_is_waiting() {
        // A node throttled by the session cap (Waiting) is pending forward
        // progress, so a stale sibling does not make the run stalled.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "capped" }),
            ),
            make_event(EventKind::NodeStarted, Some("a"), Some(1)),
            make_event(EventKind::NodeStale, Some("a"), Some(1)),
            make_event(EventKind::NodeWaiting, Some("b"), Some(1)),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.nodes["b"].status, NodeStatus::Waiting);
        assert!(!is_stalled(&state), "a waiting node is not a stall");
    }

    #[test]
    fn stalled_clears_when_stale_node_resumes() {
        // AC: "A Run that resumes activity leaves the stale state." The same
        // node restarting (e.g. manual retry) flips it back to Running.
        let mut events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "recover" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
        ];
        assert!(is_stalled(&project(&events).unwrap()));

        events.push(make_event(EventKind::NodeStarted, Some("worker"), Some(2)));
        let state = project(&events).unwrap();
        assert_eq!(state.nodes["worker"].status, NodeStatus::Running);
        assert!(
            !is_stalled(&state),
            "resumed activity clears the stalled overlay"
        );
    }

    #[test]
    fn not_stalled_without_any_stale_node() {
        // A plain mid-execution run (running node, no stale) is not stalled.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "healthy" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
        ];
        assert!(!is_stalled(&project(&events).unwrap()));
    }

    #[test]
    fn not_stalled_when_paused() {
        // A paused run with a stale node is intentionally idle, not stalled.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "paused" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
            make_event(EventKind::RunPaused, None, None),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Paused);
        assert!(!is_stalled(&state), "a paused run is never stalled");
    }

    #[test]
    fn not_stalled_when_merge_resolver_active() {
        // A running merge resolver is forward progress even if a node is stale.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "merging" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStale, Some("worker"), Some(1)),
            make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({ "conflicting_node_id": "worker", "iter": 1 }),
            ),
        ];
        let state = project(&events).unwrap();
        assert_eq!(
            state.merge_resolver.as_ref().unwrap().status,
            NodeStatus::Running
        );
        assert!(
            !is_stalled(&state),
            "an active merge resolver means the run is still progressing"
        );
    }

    #[test]
    fn not_stalled_when_completed() {
        // A completed run has no stale nodes and a terminal status.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "done" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert!(!is_stalled(&state));
    }

    // --- Golden / characterization projection (issue #238) ---

    /// One representative event log that exercises every projection concern in a
    /// single run: run lifecycle (start/pause/resume/rename/complete), node
    /// transitions (waiting/started/completed/failed/auto-completed/stopped/
    /// stale/invalidated/awaiting-user/frontmatter-retry), switch routing, a
    /// bounded loop region, two foreach barriers, the merge resolver, the
    /// passive pipeline events, and the command dispatcher (end_region,
    /// resume_run, unknown). Used by `projection_golden` to pin the full
    /// projected `RunState` across the per-concern decomposition (#238).
    fn golden_event_log() -> Vec<Event> {
        fn ev(
            kind: EventKind,
            node_id: Option<&str>,
            iter: Option<i64>,
            ts: &str,
            payload: Option<serde_json::Value>,
        ) -> Event {
            Event {
                id: None,
                run_id: "run-golden".into(),
                ts: ts.into(),
                kind,
                node_id: node_id.map(String::from),
                iter,
                payload,
            }
        }

        vec![
            ev(
                EventKind::RunStarted,
                None,
                None,
                "2026-02-01T00:00:00.000Z",
                Some(serde_json::json!({
                    "pipeline_name": "golden-pipe",
                    "name": "Golden Run",
                    "input": "exercise every concern",
                    "image_filenames": ["screenshot.png"],
                    "target_repo": "Loulen/prompt-driven-orchestrator",
                    "source_branch": "main",
                    "triggered_by": "trigger-7",
                    "node_defs": [
                        start_node_def(),
                        end_node_def(),
                        node_def("planner"),
                        node_def("worker"),
                        node_def("auto"),
                        node_def("stopped"),
                        node_def("stale"),
                        node_def("temp"),
                        node_def("interactive"),
                        node_def("sw"),
                    ],
                    "edges": [
                        edge_info("start", "planner"),
                        edge_info("planner", "worker"),
                        edge_info("worker", "end"),
                    ],
                })),
            ),
            // Loop region region1: iter started, break received, max reached
            // (informational), then done.
            ev(
                EventKind::LoopIterStarted,
                None,
                None,
                "2026-02-01T00:00:01.000Z",
                Some(serde_json::json!({ "loop_node_id": "region1", "iter": 2, "max_iter": 3 })),
            ),
            ev(
                EventKind::LoopBreakReceived,
                None,
                None,
                "2026-02-01T00:00:02.000Z",
                Some(serde_json::json!({ "loop_node_id": "region1" })),
            ),
            ev(
                EventKind::LoopMaxReached,
                None,
                None,
                "2026-02-01T00:00:03.000Z",
                Some(serde_json::json!({ "loop_node_id": "region1" })),
            ),
            ev(
                EventKind::LoopDone,
                None,
                None,
                "2026-02-01T00:00:04.000Z",
                Some(serde_json::json!({ "loop_node_id": "region1" })),
            ),
            // ForEach fe1: started -> break received -> done.
            ev(
                EventKind::ForEachStarted,
                None,
                None,
                "2026-02-01T00:00:05.000Z",
                Some(serde_json::json!({ "foreach_node_id": "fe1", "total_items": 2 })),
            ),
            ev(
                EventKind::ForEachBreakReceived,
                None,
                None,
                "2026-02-01T00:00:06.000Z",
                Some(serde_json::json!({ "foreach_node_id": "fe1" })),
            ),
            ev(
                EventKind::ForEachDone,
                None,
                None,
                "2026-02-01T00:00:07.000Z",
                Some(serde_json::json!({ "foreach_node_id": "fe1" })),
            ),
            // ForEach fe2: empty list short-circuits to done.
            ev(
                EventKind::ForEachEmpty,
                None,
                None,
                "2026-02-01T00:00:08.000Z",
                Some(serde_json::json!({ "foreach_node_id": "fe2" })),
            ),
            // planner: waiting -> started -> completed, plus a frontmatter retry.
            ev(
                EventKind::NodeWaiting,
                Some("planner"),
                Some(1),
                "2026-02-01T00:01:00.000Z",
                None,
            ),
            ev(
                EventKind::NodeStarted,
                Some("planner"),
                Some(1),
                "2026-02-01T00:01:01.000Z",
                None,
            ),
            ev(
                EventKind::FrontmatterRetryPending,
                Some("planner"),
                Some(1),
                "2026-02-01T00:01:02.000Z",
                None,
            ),
            ev(
                EventKind::NodeCompleted,
                Some("planner"),
                Some(1),
                "2026-02-01T00:01:03.000Z",
                None,
            ),
            // worker: iter1 fails (with violations), iter2 completes -> the
            // node-level status follows the latest iter.
            ev(
                EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                "2026-02-01T00:02:00.000Z",
                None,
            ),
            ev(
                EventKind::NodeFailed,
                Some("worker"),
                Some(1),
                "2026-02-01T00:02:01.000Z",
                Some(serde_json::json!({
                    "reason": "output validation failed",
                    "violations": [
                        { "port": "out", "field": "verdict", "reason": "not allowed" }
                    ]
                })),
            ),
            ev(
                EventKind::NodeStarted,
                Some("worker"),
                Some(2),
                "2026-02-01T00:02:02.000Z",
                None,
            ),
            ev(
                EventKind::NodeCompleted,
                Some("worker"),
                Some(2),
                "2026-02-01T00:02:03.000Z",
                None,
            ),
            // auto: auto-completed. stopped: stopped. stale: stale.
            ev(
                EventKind::NodeStarted,
                Some("auto"),
                Some(1),
                "2026-02-01T00:03:00.000Z",
                None,
            ),
            ev(
                EventKind::NodeAutoCompleted,
                Some("auto"),
                Some(1),
                "2026-02-01T00:03:01.000Z",
                None,
            ),
            ev(
                EventKind::NodeStarted,
                Some("stopped"),
                Some(1),
                "2026-02-01T00:03:02.000Z",
                None,
            ),
            ev(
                EventKind::NodeStopped,
                Some("stopped"),
                Some(1),
                "2026-02-01T00:03:03.000Z",
                Some(serde_json::json!({ "reason": "user killed it" })),
            ),
            ev(
                EventKind::NodeStarted,
                Some("stale"),
                Some(1),
                "2026-02-01T00:03:04.000Z",
                None,
            ),
            ev(
                EventKind::NodeStale,
                Some("stale"),
                Some(1),
                "2026-02-01T00:03:05.000Z",
                None,
            ),
            // temp: started then invalidated -> removed from state entirely.
            ev(
                EventKind::NodeStarted,
                Some("temp"),
                Some(1),
                "2026-02-01T00:03:06.000Z",
                None,
            ),
            ev(
                EventKind::NodeInvalidated,
                Some("temp"),
                None,
                "2026-02-01T00:03:07.000Z",
                None,
            ),
            // interactive: started -> awaiting user -> completed.
            ev(
                EventKind::NodeStarted,
                Some("interactive"),
                Some(1),
                "2026-02-01T00:04:00.000Z",
                None,
            ),
            ev(
                EventKind::NodeAwaitingUser,
                Some("interactive"),
                Some(1),
                "2026-02-01T00:04:01.000Z",
                None,
            ),
            ev(
                EventKind::NodeCompleted,
                Some("interactive"),
                Some(1),
                "2026-02-01T00:04:02.000Z",
                None,
            ),
            // switch routing -> synthetic completed node + switch_state.
            ev(
                EventKind::SwitchRouted,
                Some("sw"),
                Some(1),
                "2026-02-01T00:05:00.000Z",
                Some(serde_json::json!({ "node_id": "sw", "chosen_branch": "pass" })),
            ),
            // merge resolver: conflict -> started -> completed.
            ev(
                EventKind::MergeConflictDetected,
                Some("worker"),
                Some(2),
                "2026-02-01T00:06:00.000Z",
                Some(serde_json::json!({ "reason": "conflict merging worker" })),
            ),
            ev(
                EventKind::MergeResolverStarted,
                None,
                None,
                "2026-02-01T00:06:01.000Z",
                Some(serde_json::json!({
                    "conflicting_node_id": "worker",
                    "iter": 2,
                    "session_name": "pdo-run-golden-__merge_resolver__-iter-2"
                })),
            ),
            ev(
                EventKind::MergeResolverCompleted,
                None,
                None,
                "2026-02-01T00:06:02.000Z",
                None,
            ),
            // passive pipeline events (informational / terminal-safe).
            ev(
                EventKind::PipelineLint,
                None,
                None,
                "2026-02-01T00:07:00.000Z",
                None,
            ),
            ev(
                EventKind::PipelineModified,
                None,
                None,
                "2026-02-01T00:07:01.000Z",
                None,
            ),
            // pause/resume round-trip mid-run.
            ev(
                EventKind::RunPaused,
                None,
                None,
                "2026-02-01T00:08:00.000Z",
                None,
            ),
            ev(
                EventKind::RunResumed,
                None,
                None,
                "2026-02-01T00:08:01.000Z",
                None,
            ),
            // command dispatcher: end_region (creates a closed region2 loop
            // state), resume_run (no-op on a Running run), unknown (no-op).
            ev(
                EventKind::CommandIssued,
                None,
                None,
                "2026-02-01T00:09:00.000Z",
                Some(serde_json::json!({ "command": "end_region", "region_id": "region2" })),
            ),
            ev(
                EventKind::CommandIssued,
                None,
                None,
                "2026-02-01T00:09:01.000Z",
                Some(serde_json::json!({ "command": "resume_run" })),
            ),
            ev(
                EventKind::CommandIssued,
                None,
                None,
                "2026-02-01T00:09:02.000Z",
                Some(serde_json::json!({ "command": "totally_unknown" })),
            ),
            // rename then terminal completion.
            ev(
                EventKind::RunRenamed,
                None,
                None,
                "2026-02-01T00:10:00.000Z",
                Some(serde_json::json!({ "name": "Golden Run (final)" })),
            ),
            ev(
                EventKind::RunCompleted,
                None,
                None,
                "2026-02-01T00:11:00.000Z",
                None,
            ),
        ]
    }

    /// Golden characterization (#238, AC#3): the full projected `RunState` for a
    /// representative event log, pinned byte-for-byte across the per-concern
    /// decomposition. We compare `serde_json::to_value(&state)` (a `BTreeMap`-
    /// backed, sorted-key `Value` — `serde_json` has no `preserve_order` here, so
    /// `HashMap` iteration order cannot flake the comparison) against an inline
    /// expected literal captured against the pre-refactor monolith. If this
    /// snapshot ever changes, the projection's behavior changed — investigate
    /// rather than re-baseline. The expected literal is intentionally exhaustive
    /// (every concern's contribution to the state is present) so that any
    /// per-applier regression surfaces here.
    #[test]
    fn projection_golden() {
        let state = project(&golden_event_log()).unwrap();
        let actual = serde_json::to_value(&state).unwrap();
        let expected = serde_json::json!({
            "completed_at": "2026-02-01T00:11:00.000Z",
            "edges": [
                { "source_node": "start", "source_port": "out", "target_node": "planner", "target_port": "task" },
                { "source_node": "planner", "source_port": "out", "target_node": "worker", "target_port": "task" },
                { "source_node": "worker", "source_port": "out", "target_node": "end", "target_port": "task" }
            ],
            "end_node": {
                "id": "end",
                "ports": [
                    { "fired_at": "2026-02-01T00:11:00.000Z", "port_name": "result", "reason": null, "status": "received" }
                ]
            },
            "foreach_states": {
                "fe1": { "break_received": true, "done": true, "foreach_node_id": "fe1", "total_items": 2 },
                "fe2": { "break_received": false, "done": true, "foreach_node_id": "fe2", "total_items": 0 }
            },
            "input": "exercise every concern",
            "loop_states": {
                "region1": { "break_received": true, "current_iter": 2, "done": true, "loop_node_id": "region1", "max_iter": 3 },
                "region2": { "break_received": false, "current_iter": 1, "done": true, "loop_node_id": "region2", "max_iter": 0 }
            },
            "merge_resolver": {
                "completed_at": "2026-02-01T00:06:02.000Z",
                "conflicting_node_id": "worker",
                "failure_reason": null,
                "iter": 2,
                "session_name": "pdo-run-golden-__merge_resolver__-iter-2",
                "started_at": "2026-02-01T00:06:01.000Z",
                "status": "completed"
            },
            "name": "Golden Run (final)",
            "node_defs": [
                { "id": "start", "inputs": [], "node_type": "start", "outputs": [ { "name": "user_prompt", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "end", "inputs": [ { "name": "result", "side": "left" } ], "node_type": "end", "outputs": [], "view_x": null, "view_y": null },
                { "id": "planner", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "worker", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "auto", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "stopped", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "stale", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "temp", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "interactive", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "sw", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "doc-only", "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null }
            ],
            "nodes": {
                "auto": {
                    "completed_at": "2026-02-01T00:03:01.000Z", "failure_reason": null, "frontmatter_retries": 0, "iter": 1,
                    "iterations": [ { "completed_at": "2026-02-01T00:03:01.000Z", "iter": 1, "started_at": "2026-02-01T00:03:00.000Z", "status": "completed" } ],
                    "node_id": "auto", "started_at": "2026-02-01T00:03:00.000Z", "status": "completed"
                },
                "interactive": {
                    "completed_at": "2026-02-01T00:04:02.000Z", "failure_reason": null, "frontmatter_retries": 0, "iter": 1,
                    "iterations": [ { "completed_at": "2026-02-01T00:04:02.000Z", "iter": 1, "started_at": "2026-02-01T00:04:00.000Z", "status": "completed" } ],
                    "node_id": "interactive", "started_at": "2026-02-01T00:04:00.000Z", "status": "completed"
                },
                "planner": {
                    "completed_at": "2026-02-01T00:01:03.000Z", "failure_reason": null, "frontmatter_retries": 1, "iter": 1,
                    "iterations": [ { "completed_at": "2026-02-01T00:01:03.000Z", "iter": 1, "started_at": "2026-02-01T00:01:01.000Z", "status": "completed" } ],
                    "node_id": "planner", "started_at": "2026-02-01T00:01:01.000Z", "status": "completed"
                },
                "stale": {
                    "completed_at": null, "failure_reason": null, "frontmatter_retries": 0, "iter": 1,
                    "iterations": [ { "completed_at": null, "iter": 1, "started_at": "2026-02-01T00:03:04.000Z", "status": "stale" } ],
                    "node_id": "stale", "started_at": "2026-02-01T00:03:04.000Z", "status": "stale"
                },
                "stopped": {
                    "completed_at": "2026-02-01T00:03:03.000Z", "failure_reason": "user killed it", "frontmatter_retries": 0, "iter": 1,
                    "iterations": [ { "completed_at": "2026-02-01T00:03:03.000Z", "iter": 1, "started_at": "2026-02-01T00:03:02.000Z", "status": "stopped" } ],
                    "node_id": "stopped", "started_at": "2026-02-01T00:03:02.000Z", "status": "stopped"
                },
                "sw": {
                    "completed_at": "2026-02-01T00:05:00.000Z", "failure_reason": null, "frontmatter_retries": 0, "iter": 1,
                    "iterations": [ { "completed_at": "2026-02-01T00:05:00.000Z", "iter": 1, "started_at": "2026-02-01T00:05:00.000Z", "status": "completed" } ],
                    "node_id": "sw", "started_at": "2026-02-01T00:05:00.000Z", "status": "completed"
                },
                "worker": {
                    "completed_at": "2026-02-01T00:02:03.000Z", "failure_reason": null, "frontmatter_retries": 0,
                    "frontmatter_violations": [ { "field": "verdict", "port": "out", "reason": "not allowed" } ],
                    "iter": 2,
                    "iterations": [
                        { "completed_at": "2026-02-01T00:02:01.000Z", "iter": 1, "started_at": "2026-02-01T00:02:00.000Z", "status": "failed" },
                        { "completed_at": "2026-02-01T00:02:03.000Z", "iter": 2, "started_at": "2026-02-01T00:02:02.000Z", "status": "completed" }
                    ],
                    "node_id": "worker", "started_at": "2026-02-01T00:02:02.000Z", "status": "completed"
                }
            },
            "pipeline_name": "golden-pipe",
            "run_id": "run-golden",
            "sessions_spawned": 8,
            "source_branch": "main",
            "start_node": {
                "input_images": [ "screenshot.png" ],
                "input_path": "_input/output.md",
                "started_at": "2026-02-01T00:00:00.000Z",
                "target_node_ids": [ "planner" ]
            },
            "started_at": "2026-02-01T00:00:00.000Z",
            "status": "completed",
            "switch_states": {
                "sw": { "chosen_branch": "pass", "evaluated_at": "2026-02-01T00:05:00.000Z", "switch_node_id": "sw" }
            },
            "target_repo": "Loulen/prompt-driven-orchestrator",
            "triggered_by": "trigger-7"
        });
        assert_eq!(actual, expected);
    }

    // --- Focused per-applier unit tests (#238, AC#2) ---
    // Each sub-applier folds one event into a bare `RunState` in isolation — no
    // full run, no `RunStarted` bootstrap — proving the decomposition is
    // independently unit-testable as the issue requires.

    #[test]
    fn apply_loop_event_accounts_a_lap_without_a_full_run() {
        // AC#2's named example: loop-lap accounting without a full run. Fold a
        // single `LoopIterStarted` into a bare state and assert the loop_state,
        // then close it with `LoopDone` — all with no surrounding run.
        let mut state = RunState::new("r".into(), String::new());
        apply_loop_event(
            &mut state,
            &make_event_with_payload(
                EventKind::LoopIterStarted,
                None,
                serde_json::json!({ "loop_node_id": "L", "iter": 3, "max_iter": 5 }),
            ),
        );
        let ls = &state.loop_states["L"];
        assert_eq!(ls.current_iter, 3);
        assert_eq!(ls.max_iter, 5);
        assert!(!ls.done);

        apply_loop_event(
            &mut state,
            &make_event_with_payload(
                EventKind::LoopDone,
                None,
                serde_json::json!({ "loop_node_id": "L" }),
            ),
        );
        assert!(state.loop_states["L"].done);
    }

    #[test]
    fn apply_node_event_opens_an_iteration_in_isolation() {
        let mut state = RunState::new("r".into(), String::new());
        apply_node_event(
            &mut state,
            &make_event(EventKind::NodeStarted, Some("n"), Some(1)),
        );
        assert_eq!(state.nodes["n"].status, NodeStatus::Running);
        assert_eq!(state.sessions_spawned, 1);
        assert_eq!(state.nodes["n"].iterations.len(), 1);
    }

    #[test]
    fn apply_foreach_event_tracks_total_items_in_isolation() {
        let mut state = RunState::new("r".into(), String::new());
        apply_foreach_event(
            &mut state,
            &make_event_with_payload(
                EventKind::ForEachStarted,
                None,
                serde_json::json!({ "foreach_node_id": "fe", "total_items": 4 }),
            ),
        );
        assert_eq!(state.foreach_states["fe"].total_items, 4);
        assert!(!state.foreach_states["fe"].done);
    }

    #[test]
    fn apply_command_event_end_region_closes_region_in_isolation() {
        let mut state = RunState::new("r".into(), String::new());
        apply_command_event(
            &mut state,
            &make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "end_region", "region_id": "R" }),
            ),
        );
        assert!(state.loop_states["R"].done);
    }

    #[test]
    fn apply_merge_event_runs_resolver_lifecycle_in_isolation() {
        let mut state = RunState::new("r".into(), String::new());
        apply_merge_event(
            &mut state,
            &make_event_with_payload(
                EventKind::MergeResolverStarted,
                None,
                serde_json::json!({ "conflicting_node_id": "x", "iter": 1 }),
            ),
        );
        assert_eq!(
            state.merge_resolver.as_ref().unwrap().status,
            NodeStatus::Running
        );
        apply_merge_event(
            &mut state,
            &make_event(EventKind::MergeResolverCompleted, None, None),
        );
        assert_eq!(
            state.merge_resolver.as_ref().unwrap().status,
            NodeStatus::Completed
        );
    }

    #[test]
    fn appliers_never_panic_on_a_misrouted_kind() {
        // D5 hard rule: an applier must never panic, even if handed a kind it
        // does not own — its inner match's `_ => {}` swallows it. `project()`
        // relies on this never crashing, because it also runs inside
        // `append_event` before the transition guard. Here `apply_run_event` is
        // handed a `NodeStarted` (owned by `apply_node_event`): it must no-op.
        let mut state = RunState::new("r".into(), String::new());
        apply_run_event(
            &mut state,
            &make_event(EventKind::NodeStarted, Some("n"), Some(1)),
        );
        assert!(state.nodes.is_empty(), "misrouted kind must be a no-op");
        assert_eq!(state.status, RunStatus::Running);
    }
}
