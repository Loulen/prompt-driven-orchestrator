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
use tracing::warn;

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
    /// Where this node works (#653, ADR-0060), as the Run's pipeline snapshot
    /// froze it: `true` ⇒ its own sub-worktree, `false` ⇒ the Run worktree.
    /// `None` for a type that carries no isolation (`merge` is isolated by
    /// construction; structural nodes have no worktree of their own) and for a
    /// pre-#653 snapshot. `skip_serializing_if` keeps the wire byte-identical
    /// when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_worktree: Option<bool>,
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
    /// An **infra** incident killed the node's session — session death, boot
    /// recovery, or a spawn-abort on the scheduler path (ADR-0049 / ADR-0050).
    /// Projects [`NodeStatus::Interrupted`] (non terminal, distinct from
    /// `NodeFailed`), which lifts the run to `AwaitingUser` with the incident
    /// reason in [`finalize`]. The runtime **never** turns this into `Failed`;
    /// only a deliberate `pdo fail` or a human abandon does.
    ///
    /// Unlike `NodeFailed`, the guard (`validate_interrupt`) admits it even for
    /// an iteration that never opened a `NodeStarted` row — a spawn that aborts
    /// *before* start still names its node and cause (ADR-0050 §1), and the
    /// projection materialises the node so the incident is visible. Wire form:
    /// `"node_interrupted"`.
    NodeInterrupted,
    /// PDO delivered a NodeRun's work onto the Run's branch (#654 / ADR-0060):
    /// its own commits kept, whatever it left behind committed under
    /// `<node-id> iter-<N>: completed`, then merged back if it was isolated.
    ///
    /// Written **only when the branch actually moved** — a NodeRun that left
    /// nothing writes no commit and no event — and always *before* the terminal
    /// completion event, so "delivered, then done" is the order the log reads in.
    /// Payload: `before` / `after`, the two Run-branch tips, projected onto
    /// [`NodeState::delivery`]. Wire form: `"node_delivered"`.
    NodeDelivered,
    MergeConflictDetected,
    /// A merge-back conflicted and was resolved **in the node's favour** instead of
    /// failing the Run (#503, ADR-0036): the node's branch had stopped being a
    /// descendant of the pipeline branch (a terminal node rebasing onto a moved
    /// integration branch does that), and nothing else had reached the pipeline
    /// branch since the node was cut from it, so the divergence was the run's own
    /// history rewritten by the node.
    ///
    /// Informational in projection — the completion continues normally — but never
    /// silent: PDO rewrote a branch, and the payload carries the two tips, the
    /// resolution commit and what would have conflicted. Wire form:
    /// `"merge_resolved_in_node_favour"`.
    MergeResolvedInNodeFavour,
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
    /// A `kind: collection` loop region resolved its `over` list and fanned its
    /// entry out, one lap per item (ADR-0011 / #269). Keyed by region id.
    CollectionStarted,
    /// The region's `over` list resolved empty: the barrier fires immediately
    /// with zero item laps (ADR-0011 / #269).
    CollectionEmpty,
    /// Every item lap of the collection region completed — the barrier fired
    /// (ADR-0011 / #269).
    CollectionDone,
    NodeStopped,
    NodeAutoCompleted,
    NodeStale,
    NodeInvalidated,
    /// Informational (#290): a node's Claude Code session is blocked on the
    /// usage-limit interactive menu (host-level; session alive, no progress).
    /// Behaviour-preserving no-op in projection — the node stays Running;
    /// recovery is deferred (Slice 2/3). Wire form: `"node_blocked_on_limit"`.
    NodeBlockedOnLimit,
    /// Informational, **no producer since #469**. Don't delete the variant: the
    /// log is append-only, and a Run that recorded one before #469 would fail to
    /// deserialise, so `project()` would return `None` and the Run would vanish
    /// from the UI. No-op in projection. Wire form: `"node_auto_complete_observed"`.
    NodeAutoCompleteObserved,
    PipelineLint,
    PipelineModified,
    RunCompleted,
    RunFailed,
    /// The runtime **gave up** on driving the run forward for a reason that is
    /// **not** a deliberate failure — a run-level stall, an output-validation
    /// refusal, a merge conflict, or an `unrouted` convergence (ADR-0049). It
    /// parks the run **`AwaitingUser`** with the reason carried in
    /// [`RunState::awaiting_reason`], **never `RunFailed`**: a human confirms,
    /// reopens, or drives it out. **Non terminal** — it never sets
    /// `completed_at`, and it is inert on an already-terminal run (#221: an
    /// active give-up must not un-terminalize a run that genuinely finished).
    /// The run-level twin of [`NodeInterrupted`], for the give-up cases that
    /// name no single node to interrupt. Wire form: `"run_interrupted"`.
    RunInterrupted,
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
    /// The Run's read-only secondary list was edited mid-run (#465 slice 2,
    /// ADR-0042). The payload carries the **complete, re-frozen** active list under
    /// `target_repos` (a `Vec<RepoPin>`, SHAs already resolved and aliases already
    /// disambiguated by the handler) — never raw input, so replay re-resolves
    /// nothing, exactly like `RunStarted`. The reducer overwrites
    /// `RunState::target_repos` wholesale (idempotent, order-independent), and is a
    /// strict no-op on a terminal Run (#221 — a passive metadata event must never
    /// un-terminalize a Run). Absent on every mono-repo and every Run that never
    /// edited its list, so those payloads stay byte-identical. Wire form:
    /// `"run_repos_edited"`.
    RunReposEdited,
    /// Informational (#410): a sandboxed Run's image is being prepared (pull/build)
    /// at the head of the detached prep task, before the first session spawns. Emitted
    /// only on the create path and only when the resolved mode is `full`/`minimal` (the
    /// `off` path stays byte-identical). Non-terminal: `status` stays `Running`, only
    /// `RunState::sandbox_prep` moves to `pending`. Wire form: `"sandbox_prep_started"`.
    SandboxPrepStarted,
    /// Informational (#410): the sandbox image is ready and the container is about to
    /// receive the first session. Projects `RunState::sandbox_prep` to `ready`. A
    /// prep failure emits `RunFailed` instead (no dedicated failed-prep event). Wire
    /// form: `"sandbox_prep_ready"`.
    SandboxPrepReady,
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

/// How a Run is isolated (#403 / #407 / #432). A **per-Run, immutable** property
/// carried on `RunStarted`, projected once into [`RunState::sandbox`], never mutated
/// for the Run's whole life — a resumed session matches its transcript by working-dir
/// path, so flipping the mode mid-life would break `claude --continue`.
///
/// `full` and `minimal` are *virtual defaults* (no DB row until edited), so they keep
/// round-tripping byte-identically through every historical payload.
///
/// The wire form is a **bare string**: `off`, or the profile name verbatim. Serde is
/// hand-written for exactly that reason — `untagged` would emit `null` for the unit
/// variant, and `#[serde(from = "String")]` would demand an infallible conversion
/// while a blank token must fail.
///
/// [`SandboxMode::parse`] is purely **syntactic**: `None` means *blank*, not
/// *unknown*. Whether a profile **exists** is a database question, answered at the
/// edge (create-run, `PUT /settings`, trigger create/patch) and never here — this
/// module is pure and its projection runs inside `append_event`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum SandboxMode {
    #[default]
    Off,
    /// Canonicalised by [`SandboxMode::parse`]: trimmed, never empty, never `off`.
    Profile(String),
}

/// Wire tokens of a dropped two-position switch. Don't remove this mapping: `parse`
/// accepts any non-blank token as a profile name, so a historical payload carrying
/// one would project to `Profile("copy")` — an unknown profile — and fail the Run
/// hard. NOT consulted anywhere the user can still type a value: a stored `copy`
/// fails loud at the create chokepoint (ADR-0031 §7).
const LEGACY_SANDBOX_TOKENS: &[&str] = &["copy", "pure"];

impl SandboxMode {
    /// The default tier (never `None`), surfaced by `GET /settings` and used as the
    /// precedence floor: an install with no `default_sandbox` set runs `Off`, so the
    /// legacy host path stays byte-identical (#410). Mirror of [`crate::sandbox_image::ImageSource::DEFAULT`].
    pub const DEFAULT: SandboxMode = SandboxMode::Off;

    /// The wire token of [`SandboxMode::Off`]. A `const &str` rather than
    /// `DEFAULT.as_str()` because `as_str` now borrows from `self` (the profile name
    /// is owned), which no `const fn` can do.
    pub const OFF_WIRE: &'static str = "off";

    /// Whether this Run runs on the host (the legacy, no-Docker path). The whole
    /// sandbox wiring is gated on `!is_off()`, so the `off` parcours never touches
    /// a single new line.
    pub fn is_off(&self) -> bool {
        matches!(self, SandboxMode::Off)
    }

    /// The exact wire form: `off`, or the profile name verbatim. Consumed by
    /// `build_settings_view` and the enum validators (#410).
    pub fn as_str(&self) -> &str {
        match self {
            SandboxMode::Off => Self::OFF_WIRE,
            SandboxMode::Profile(name) => name.as_str(),
        }
    }

    /// The staging profile this Run uses, or `None` for `off`. The ONE consumer that
    /// needs the name rather than the off-ness (#432 D2) is the sandbox context
    /// assembly, which resolves it to a frozen entry list.
    pub fn profile(&self) -> Option<&str> {
        match self {
            SandboxMode::Off => None,
            SandboxMode::Profile(name) => Some(name.as_str()),
        }
    }

    /// Parse the wire form. **Purely syntactic** (#432): `off` (case/whitespace
    /// tolerant, the three closed tokens of the old enum are gone) yields `Off`, a
    /// blank string yields `None`, and anything else is a profile name — trimmed but
    /// otherwise **verbatim**, never lowercased.
    ///
    /// The asymmetry with [`crate::sandbox_profile::validate_profile_name`] (which
    /// rejects an uppercase name outright instead of folding it) is deliberate and
    /// load-bearing: `off` is a closed token whose spelling nobody owns, while a
    /// profile name is a user namespace where accepting `Foo` and silently storing
    /// `foo` would make the UI search a list it does not display. Do not "fix" it.
    pub fn parse(s: &str) -> Option<SandboxMode> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.eq_ignore_ascii_case(Self::OFF_WIRE) {
            return Some(SandboxMode::Off);
        }
        Some(SandboxMode::Profile(trimmed.to_string()))
    }
}

impl Serialize for SandboxMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SandboxMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        SandboxMode::parse(&raw).ok_or_else(|| {
            serde::de::Error::custom(
                "sandbox must be `off` or the name of a staging profile, not blank",
            )
        })
    }
}

/// Env var overriding the stored instance default (optional tier). Read ONCE at the
/// edge (create-run chokepoint + `build_settings_view` disclosure), never in the
/// resolver core — mirror of [`crate::sandbox_image::IMAGE_SOURCE_ENV`] (#410).
pub(crate) const DEFAULT_SANDBOX_ENV: &str = "PDO_DEFAULT_SANDBOX";

/// Env tier for the settings disclosure / resolver: `Some(mode)` if a valid
/// `PDO_DEFAULT_SANDBOX` is set, else `None` (unset/invalid).
fn env_default_sandbox() -> Option<SandboxMode> {
    std::env::var(DEFAULT_SANDBOX_ENV)
        .ok()
        .as_deref()
        .and_then(SandboxMode::parse)
}

/// Instance default, precedence `stored → env → default(Off)`. A stored empty value is
/// treated as unset. SINGLE source shared by `create_run_inner` AND
/// `build_settings_view`.
///
/// A stored profile name that does not *exist* is deliberately NOT demoted to `off`
/// here: it wins the tier and the create-run chokepoint 400s on it by name (ADR-0031
/// §7 — never a silent fallback toward less isolation).
pub(crate) fn default_sandbox_with(stored: Option<String>) -> SandboxMode {
    stored
        .filter(|s| !s.is_empty())
        .as_deref()
        .and_then(SandboxMode::parse)
        .or_else(env_default_sandbox)
        .unwrap_or(SandboxMode::DEFAULT)
}

/// Precedence resolver (#410): `explicit → trigger → instance_default` (first `Some`
/// wins, `instance_default` is the floor). Pure — no `AppState`/DB/Docker in scope;
/// this is the layer-1 unit the "précédence testée" AC pins. `explicit` and `trigger`
/// are mutually exclusive in production (a Run has one origin), but the 3-arg form is
/// the canonical statement of the chain and keeps every arm exercised by the test.
pub(crate) fn effective_sandbox(
    explicit: Option<SandboxMode>,
    trigger: Option<SandboxMode>,
    instance_default: SandboxMode,
) -> SandboxMode {
    explicit.or(trigger).unwrap_or(instance_default)
}

/// Visibility of a sandboxed Run's one-time image preparation (#410). Additive to
/// [`RunState`]: `status` is untouched (stays `Running`), so no status consumer is
/// affected. Absent for `off` Runs and for historical/host runs. Projected from the
/// additive [`EventKind::SandboxPrepStarted`]/[`EventKind::SandboxPrepReady`] pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPrepState {
    /// The image is being pulled/built and the container has not yet received a session.
    Pending,
    /// The image is ready; the first session is about to spawn (or already has).
    Ready,
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
    /// The node was **auto-skipped** as structurally unreachable (#620): its
    /// producing branch was not taken, so nothing would ever spawn it, and the
    /// resilience sweep pruned it with an empty output rather than let the run
    /// hang. **Terminal and satisfied**, exactly like `Completed` for every
    /// scheduling gate (run-completion, re-spawn refusal, upstream-completion /
    /// reachability) — see [`NodeStatus::is_settled_complete`]. It is a distinct
    /// variant ONLY so the UI can grey it out and show *why* it was pruned,
    /// instead of the green "done" cadre a real success wears (a pruned branch
    /// must not read as a branch that ran). The node never held a session, so it
    /// carries no transient `Running` window. Projected from a `NodeCompleted`
    /// carrying `skipped: true` (`sweep_auto_skips` / `skip_node`).
    Skipped,
    Failed,
    Stopped,
    Stale,
    /// The node's session died on an **infra** incident (session death, boot
    /// recovery, spawn-abort) — "la session est morte, pas le travail"
    /// (ADR-0049). **Non terminal** and distinct from `Failed`: the work on
    /// disk is presumed intact, the run parks `AwaitingUser` (never `Failed`),
    /// and a human recovers it (resume-in-worktree or restart-with-artefacts).
    /// Holds no session and cannot progress on its own — a human gesture is
    /// required to reach `Running` again. `Failed` stays reserved for a
    /// deliberate `pdo fail` or a human abandon.
    Interrupted,
}

impl NodeStatus {
    /// Whether a node in this status currently holds a live NodeRun tmux
    /// session, and therefore consumes a global admission slot.
    /// `{Running, AwaitingUser}` (an interactive node keeps its tmux session
    /// attachable indefinitely). EXCLUDES `Waiting`: a throttled node is ready
    /// to run but has *not* spawned a session yet, so it holds no slot (#159).
    /// EXCLUDES `Interrupted`: an infra incident killed the session (ADR-0049),
    /// so a slot it once held is already freed.
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
    /// niveau Run). EXCLUDES `Interrupted`: an interrupted node needs a human to
    /// resume/restart it, so it does NOT keep the run schedulable — instead the
    /// run parks `AwaitingUser`, derived in [`finalize`].
    pub fn can_progress(&self) -> bool {
        matches!(
            self,
            NodeStatus::Running | NodeStatus::Waiting | NodeStatus::AwaitingUser
        )
    }

    /// Whether a node in this status was **interrupted by an infra incident**
    /// (ADR-0049) — the one node status that lifts a `Running` run to
    /// `AwaitingUser` with a reason (see [`finalize`]), distinct from the
    /// interactive `AwaitingUser` wait.
    pub fn is_interrupted(&self) -> bool {
        matches!(self, NodeStatus::Interrupted)
    }

    /// Whether this status is a **terminal, satisfied completion** — the node has
    /// discharged its obligation and every scheduling gate should treat it as done:
    /// run-completion (`all_nodes_completed`), re-spawn/reopen refusal
    /// (`transition_guard`), upstream-completion / reachability
    /// (`check_all_upstream_completed`, `edge_is_dead`, loop/collection barriers).
    ///
    /// `{Completed, Skipped}` (#620). A `Skipped` node never ran — its branch was
    /// not taken and the resilience sweep pruned it with an empty output — but it is
    /// as *settled* as a `Completed` one and must satisfy the same gates, or the run
    /// would hang waiting on a node that can never spawn. The two differ ONLY in
    /// display (green "done" vs greyed "skipped"); every semantic predicate that
    /// asks "is this node done?" answers yes for both. EXCLUDES the error-ish
    /// terminals (`Failed`/`Stopped`/`Stale`/`Interrupted`), which the
    /// stall / fail-fast / incident paths own.
    pub fn is_settled_complete(&self) -> bool {
        matches!(self, NodeStatus::Completed | NodeStatus::Skipped)
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
    /// #616/ADR-0046: the harness this node's session was **frozen** on at spawn,
    /// read from the `NodeStarted` payload — so the Run view shows, per node, what
    /// actually ran (the run-level default lives on [`RunState::harness`]). `None`
    /// for a node that never opened a `NodeStarted` (a pure skip) or a pre-#616
    /// snapshot; `skip_serializing_if` keeps the wire byte-identical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// #653/ADR-0060: the isolation this NodeRun was **frozen** on at spawn,
    /// read from the `NodeStarted` payload. This — not the current document — is
    /// what says where the live iteration works, so editing the graph mid-run
    /// never moves a running node between worktrees. `None` for a node that
    /// never opened a `NodeStarted`, for a structural node, or for a pre-#653
    /// snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_worktree: Option<bool>,
    /// #669/ADR-0062: the **skills effectifs** this NodeRun was frozen with at
    /// spawn (union of the four tiers, each with its origin), read from the
    /// `NodeStarted` payload. `None` for a node that never started, a `script`
    /// node (no agent) or a pre-#669 event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) skills: Option<Vec<crate::skill_selection::EffectiveSkill>>,
    /// #669: selected ids the bank no longer knew at spawn — the node ran
    /// without them (a warning, never a failure). Absent when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) missing_skills: Vec<crate::skill_selection::MissingSkill>,
    /// #672: skills promised to this NodeRun that could not be written into its
    /// worktree (versioned homonym, occupied path, content gone) — the node ran
    /// without them. Absent when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) skipped_skills: Vec<crate::skill_delivery::SkippedSkill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub skip_reason: Option<String>,
    #[serde(default)]
    pub iterations: Vec<IterationInfo>,
    #[serde(default)]
    pub frontmatter_retries: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    pub delivery: Option<NodeDelivery>,
}

/// The two Run-branch tips one delivery moved between (#654 / ADR-0060).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeDelivery {
    /// The Run branch's tip before the delivery.
    pub before: String,
    /// The Run branch's tip after it. Never equal to `before` — a delivery that
    /// moved nothing records no event at all.
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeCost {
    pub usd: Option<f64>,
    pub form: Option<CostForm>,
    pub partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unpriced_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_reasons: Vec<String>,
    pub executions: i64,
    pub readable_executions: i64,
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

/// Barrier accounting for a `kind: collection` loop region (ADR-0011 / #269),
/// keyed by region id — the region twin of [`ForEachState`]. Tracks the
/// resolved collection size and whether the barrier has fired.
///
/// `entry` and `members` (#453) make the region's **shape** readable from the
/// projection alone, not just from the pipeline file. The transition guard needs
/// them: a collection region fans its entry out in parallel (one live lap per
/// item), which is the exact opposite of the "at most one live iteration per
/// node" invariant the guard enforces everywhere else. Carrying the shape in the
/// projection keeps `transition_guard` pure — no pipeline plumbing through every
/// caller — and scopes the exemption to the region's own members while its
/// barrier is open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionState {
    pub region_id: String,
    pub total_items: i64,
    pub done: bool,
    /// The member fanned out once per item. Empty for a `CollectionEmpty`
    /// region (no fan-out happened) and for runs whose `CollectionStarted`
    /// predates #453.
    #[serde(default)]
    pub entry: String,
    /// Every member of the region. Falls back to `[entry]` on a pre-#453
    /// payload; empty when even `entry` is unknown.
    #[serde(default)]
    pub members: Vec<String>,
}

impl CollectionState {
    /// Is `node_id` a node whose iterations this region governs? Such a node is
    /// spawned once per item, so several of its iterations are legitimately live
    /// at the same time (#453).
    pub fn governs(&self, node_id: &str) -> bool {
        self.members.iter().any(|m| m == node_id) || self.entry == node_id
    }
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

/// Estimated USD cost of a Run (#272), aggregated from the per-message token
/// `usage` in each session's Claude Code transcript × a public price table (see
/// [`crate::run_cost`]). An **estimate**, not an invoice: public list prices,
/// unpriced/new models contribute $0. Derived on read, never persisted — mirrors
/// [`LocStat`]. `None` when no transcript dir is found (UI shows "—");
/// `Some { usd: 0.0, .. }` when dirs exist but carry no priced tokens.
///
/// `PartialEq` (not `Eq`) because `usd` is `f64` — compare with a tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostStat {
    pub usd: f64,
    /// True when ≥1 session used a model absent from the price table: its tokens
    /// are excluded, so `usd` is a **lower bound**. Drives the UI "(lower bound)"
    /// affordance. Derived: `partial ⟺ !unpriced_models.is_empty()` (#425 AC#4).
    pub partial: bool,
    /// The family keys (de-dated) of every model no tier could price — the
    /// offenders `partial` used to hide (#425). Sorted and de-duplicated. Lets the
    /// UI name *which* model was excluded instead of the anonymous "an unpriced
    /// model": that anonymity is exactly how `claude-fable-5` — the priciest model
    /// — stayed invisible on `/stats/cost` for weeks. Empty ⟺ `partial == false`.
    #[serde(default)]
    pub unpriced_models: Vec<String>,
    /// The harnesses this Run launched a node on that have **no cost source**
    /// capability (#553, ADR-0045). Non-empty ⇒ the Run's cost is not honestly
    /// summable — a harness like `opencode` writes its cost where PDO does not read
    /// (its own SQLite), so adding `claude`'s real dollars to that harness's
    /// invisible $0 would be a silent under-count. So the surface shows **"—" with
    /// a reason naming these harnesses**, never a `$0`, never a mute `partial`
    /// (same "name what is missing" vein as `unpriced_models`). Sorted, deduped.
    /// Empty on every all-`claude` Run, so `usd`/`partial` mean exactly what they
    /// meant before this field existed.
    #[serde(default)]
    pub uncosted_harnesses: Vec<String>,
    /// The Run's cost **ventilated by harness** (#615, ADR-0052 §3). A mixed Run's
    /// total stays summable in dollars, but it *says* itself per harness — "X via
    /// `copilot`, Y via `claude`" — because the two halves have neither the same
    /// nature (reported vs derived) nor the same precision. One entry per harness
    /// that contributed a cost, in name order. Empty on a Run with no costable
    /// session (so `usd` is 0 and there is nothing to ventilate), and — for
    /// backward compatibility — absent on a pre-#615 serialized `CostStat`, which
    /// the surfaces render exactly as they did before (a single derived figure).
    #[serde(default)]
    pub by_harness: Vec<HarnessCost>,
}

/// Which of the two legitimate cost forms a per-harness slice is (ADR-0052 §1) —
/// so a surface can say *only under a derived figure* that it is an estimate from
/// Claude Code transcripts, and never under a reported one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostForm {
    /// PDO re-derived it from tokens × the price table (`claude`). An estimate.
    Derived,
    /// The harness counted it and PDO converted by a published constant
    /// (`copilot`). Not re-derived from tokens (ADR-0052 §2).
    Reported,
}

/// One harness's slice of a Run's cost (#615, ADR-0052 §3). Additive with the
/// others in dollars (`CostStat.usd` is their sum), but tagged with its `form` so
/// the surface never mislabels a reported figure as a Claude-Code estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessCost {
    /// The harness name (`claude`, `copilot`).
    pub harness: String,
    /// This harness's cost in USD.
    pub usd: f64,
    /// Derived or reported (drives the honesty label).
    pub form: CostForm,
    /// True when this (derived) slice excluded an unpriced model — a lower bound.
    /// Always `false` for a reported slice (it never consults the price table, so
    /// it can never be a lower bound). Empty `unpriced_models` ⟺ `!partial`.
    #[serde(default)]
    pub partial: bool,
    /// The unpriced model family keys of this (derived) slice; always empty for a
    /// reported one.
    #[serde(default)]
    pub unpriced_models: Vec<String>,
}

/// A secondary repository pinned to a Run (#465, ADR-0042).
///
/// `target_repos[0]` is the **primary** repo and mirrors the legacy scalar
/// `target_repo` exactly (ADR-0033) — it is not represented as a `RepoPin` in
/// slice 1, it stays in `target_repo`. `RepoPin` describes each **secondary**
/// (index `[1..]`): a read-only snapshot materialised by
/// `git worktree add --detach <sha>` under
/// `<primary>/.pdo/runs/<id>/repos/<alias>/`.
///
/// The `sha` is resolved once at Run start (`git rev-parse --verify` against
/// `base_branch`, no fetch → base is the LOCAL ref) and frozen in the
/// `RunStarted` payload, exactly like the sandbox freeze siblings — so a Run's
/// view of a secondary can never move under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPin {
    /// Absolute path of the secondary repository on the host.
    pub repo: String,
    /// Directory name of the snapshot under `.pdo/runs/<id>/repos/`, disambiguated
    /// on basename collision. Never derived from the basename alone at read time.
    pub alias: String,
    /// The commit the snapshot is detached at, frozen at Run start.
    pub sha: String,
    /// The ref the SHA was resolved from (default: `HEAD`, the local ref). Kept
    /// for provenance / UI display; the SHA is what is authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// Opt-in read-only flag (ADR-0047). `false` (the default) means the
    /// secondary is **writable**: the agent may modify/commit/deliver it and the
    /// completion guard tolerates a dirty tracked tree. `true` restores the
    /// ADR-0042 behaviour — the snapshot is read-only context and writing a
    /// tracked file trips `secondary_repo_dirtied` (409).
    ///
    /// Polarity: `#[serde(default)]` reads an absent key as `false`, so a
    /// historical pin (written before this flag existed) is treated as writable.
    /// This is intentional and safe — see ADR-0047 decision 2.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
}

/// serde `skip_serializing_if` helper: a `read_only == false` pin serialises
/// byte-identically to a pre-ADR-0047 pin (no `read_only` key).
fn is_false(b: &bool) -> bool {
    !*b
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
    /// Why the Run reached a non-green terminal state — the `reason` of its
    /// `RunFailed` / `RunSkipped` / `RunHalted` (#503).
    ///
    /// Set on the terminal event and cleared by `RunResumed`, mirroring
    /// `NodeState::failure_reason`: a Run being driven again must not still show
    /// last time's cause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    /// Why the Run is parked **`AwaitingUser` on an incident** (ADR-0049),
    /// distinct from the interactive `AwaitingUser` wait of a node that asked
    /// its user a question. Set by a `RunInterrupted` event (run-level give-up)
    /// or derived in [`finalize`] from an `Interrupted` node's reason; cleared
    /// by `RunResumed` and by a `reopen_run`/`resume_run` command — a run being
    /// driven again must not still show last time's incident. `None` for an
    /// interactive wait (that node's own reason lives on the node), so the two
    /// awaiting causes stay distinguishable from the run state alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_reason: Option<String>,
    /// The **machine** slug companion of [`awaiting_reason`] (#601): a stable
    /// snake_case code (`session_died`, `run_stalled`, `unrouted`,
    /// `region_exhausted`, `spawn_aborted`, `boot_recovery`, a completion-refusal
    /// slug, …) the manager and UI branch on, next to the human sentence — the
    /// same slug+prose contract as a refusal body (ADR-0035). Projected from a
    /// `RunInterrupted` event's `reason_code` payload key, or derived in
    /// [`finalize`] from an `Interrupted` node's `<code>: <prose>` reason prefix
    /// (so historical logs without the explicit key still carry a code). Cleared
    /// with [`awaiting_reason`] on resume / reopen. Every non-advancement thus
    /// carries a reason that is machine-branchable, not only prose to grep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_reason_code: Option<String>,
    pub nodes: HashMap<String, NodeState>,
    #[serde(default)]
    pub edges: Vec<EdgeInfo>,
    #[serde(default)]
    pub node_defs: Vec<NodeDefInfo>,
    /// Instance + Project + Run provisioning recipe frozen at Run creation
    /// (ADR-0061). Node rules are appended only when its isolated worktree is cut.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) provisioning_rules: Vec<crate::provisioning::ScopedRules>,
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
    pub collection_states: HashMap<String, CollectionState>,
    #[serde(default)]
    pub switch_states: HashMap<String, SwitchState>,
    /// Live loop-region cap overrides folded from `set_region_max_iter` commands
    /// (ADR-0011 / #600), keyed by region id. An **absolute** cap (last-write-wins),
    /// consulted by the scheduler in place of the region's declared `max_iter` —
    /// **uniformly for a literal and a `$var` cap** (FP #1), so an operator can grant
    /// a stuck bounded region more laps in flight without editing the YAML or
    /// restarting. Folded from the append-only log, so the raised cap survives a
    /// `reopen_run` re-projection. Empty for every run that never set one (serialized
    /// byte-identically), which is every historical run.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub region_max_iter_overrides: HashMap<String, i64>,
    /// Forced routes folded from `force_route` commands (ADR-0011 / #600), keyed by
    /// **source** — a node id OR a region id — mapping to the target node id. The
    /// scheduler spawns the target (or completes, if it is `End`) when the source
    /// completes, **short-circuiting the source's `when:` edges** — the lever for a
    /// run wedged `unrouted` because a non-`PASS` verdict reaches no live branch
    /// (FP #3). Folded from the log, so the forced exit is **not re-decided** by
    /// `when:` on the next lap or after a reopen (FP #8). Empty (and byte-identical)
    /// for every run that never forced one.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub forced_routes: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_repo: Option<String>,
    /// Secondary repositories associated with this Run in **read-only** (#465,
    /// ADR-0042). Empty for a mono-repo Run (the overwhelming majority, incl. every
    /// historical run) — `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
    /// keeps a mono-repo Run's serialized state byte-identical to the pre-#465 shape
    /// (mirror of `target_repo`'s `Option::is_none` skip); a Run with secondaries
    /// serializes them.
    ///
    /// Each entry is a [`RepoPin`] frozen at Run start; the primary repo is NOT in
    /// here (it stays in `target_repo`). Projected from the `RunStarted` payload key
    /// `target_repos`; an unreadable value degrades to empty with a `warn!`, never a
    /// panic — this applier runs before the transition guard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_repos: Vec<RepoPin>,
    /// Isolation mode for this Run (#403 / #407 / #432) — `off`, or the name of the
    /// staging profile it was launched with. Immutable: set once from the
    /// `RunStarted` payload, never mutated. Absent payload field → `Off` (the
    /// legacy host path), so historical runs and bare-API creates stay off.
    #[serde(default)]
    pub sandbox: SandboxMode,
    /// The staging profile's **resolved entry list, frozen at creation** (#432,
    /// ADR-0031 §6). Written to `RunStarted` as the sibling key `sandbox_entries`,
    /// always together with `sandbox` or not at all, so editing (or deleting) a
    /// profile can never retroactively rewrite what a Run in flight already staged.
    ///
    /// The `Option` is **load-bearing**: `Some(vec![])` is a legitimate resolution
    /// (that IS `minimal`), so a bare `Vec` would confuse "legacy payload, re-resolve"
    /// with "empty profile, stage the floor only".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_entries: Option<Vec<String>>,
    /// The raw `sandbox_entries` payload value when the key was **present but
    /// unreadable** (#432). Internal to the projection — `skip`ped on the wire — and
    /// exists only so the sandbox prep can fail LOUD with the offending value in its
    /// reason. Silently re-resolving would change what the nodes that already
    /// launched saw, which is exactly what the freeze protects against.
    #[serde(skip)]
    pub sandbox_entries_raw_error: Option<String>,
    /// The staging profile's **env, frozen at creation** (#468, ADR-0031 §8). Written to
    /// `RunStarted` as the sibling key `sandbox_env` — but, unlike `sandbox_entries`, only
    /// when it is **non-empty**. That asymmetry is deliberate: for the entries, absence
    /// ("pre-#432 daemon") and emptiness (`minimal`) had to stay distinguishable; for the
    /// env they describe the same container, so `None` and `Some(empty)` both mean "no
    /// profile env" and historical run JSON stays byte-identical.
    ///
    /// The values are on the wire, like they are in SQLite and in `docker inspect`: the
    /// sandbox is not a security boundary and this is not a secret store (ADR-0031 §8). What
    /// is forbidden is the daemon **log** — see `sandbox_profile::env_names`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_env: Option<std::collections::BTreeMap<String, String>>,
    /// The raw `sandbox_env` payload value when the key was **present but unreadable**
    /// (#468). Same role, and same `skip`, as [`RunState::sandbox_entries_raw_error`]: the
    /// sandbox prep fails LOUD with the offending value rather than silently posing a
    /// different environment than the nodes that already launched saw.
    #[serde(skip)]
    pub sandbox_env_raw_error: Option<String>,
    /// The staging profile's **image source, frozen at creation** (#467, ADR-0031 §9). Third
    /// sibling key of `sandbox_entries` / `sandbox_env`, written by the same `resolve` and — like
    /// the env, unlike the entries — **only when the profile poses one**.
    ///
    /// `None` means "this Run's profile posed no image source", indistinguishable from "a
    /// pre-#467 daemon created this Run": in both cases the **profile default** decides
    /// ([`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`]), overridable by two env vars read
    /// fresh at each prep. That is the one place the freeze is deliberately *not* total, and
    /// it is safe: a daemon's environment does not change under a running Run.
    ///
    /// What IS frozen is the profile's choice: a Run cannot have its image swapped under it
    /// because someone edited the profile, so two nodes of the same Run can never land in two
    /// different images (ADR-0031 §6).
    ///
    /// Unlike [`SandboxMode`] the value is structured (`{kind, path|ref}`), affordable here
    /// only because the key is new — no historical payload carries it as a bare string, so no
    /// reader has to accept `String | Object` for ever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_image: Option<crate::sandbox_image::ProfileImage>,
    /// The raw `sandbox_image` payload value when the key was **present but unreadable** (#467).
    /// Same role, and same `skip`, as its two twins: the sandbox prep fails LOUD with the
    /// offending value rather than silently starting a container in a *different image* than the
    /// nodes that already launched ran in.
    #[serde(skip)]
    pub sandbox_image_raw_error: Option<String>,
    /// One-time image-prep visibility for a sandboxed Run (#410). Additive: `None`
    /// for `off`/historical runs; `pending` while the image is pulled/built at first
    /// use; `ready` once the container is about to run. `status` is never touched —
    /// this drives a banner, not admission/overlap/liveness logic. Survives a daemon
    /// restart by event replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_prep: Option<SandboxPrepState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    /// The commit `pdo/run-<id>` was cut from at Run start — the run's fork point,
    /// FROZEN here from the `RunStarted` payload (#417), same immutability posture as
    /// `source_branch`/`harness`/`RepoPin.sha`. The stable 3-dot base for the LOC stat
    /// and the Run diff, so a shared checkout whose HEAD later wanders can no longer
    /// displace the merge-base and inflate the count. `None` ⇒ a pre-#417 Run (payload
    /// omits the key) → fall back to `source_branch`, then `HEAD`. NB: distinct from the
    /// per-node `NodeStarted.base_sha` (sub-worktree ← pipeline branch, ADR-0036).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_sha: Option<String>,
    /// The harness chosen at Run creation (#551, ADR-0046), **frozen** here from the
    /// `RunStarted` payload — the middle tier of the precedence chain
    /// `nœud → Run → Projet → instance → plancher (claude)`. Immutable, exactly like
    /// [`RunState::sandbox`]: set once from the create event, never mutated, so a
    /// pipeline edit or a changed instance default cannot re-decide a Run in flight.
    ///
    /// `None` ⇒ the Run named no harness, so each free node resolves through the
    /// instance default and the floor as before (every historical Run, and any Run
    /// created without an explicit choice — the payload omits the key, keeping it
    /// byte-identical to the pre-#551 shape). A blank/empty stored value collapses to
    /// `None` at the freeze (`Some("")` never persists), so it can never win a tier
    /// (#347). Read at every spawn seam as the `run` tier of [`crate::harness_resolver`]
    /// and by the infra sessions (Pipeline Manager, merge resolver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The Run tier of the agentic-profile union (#563, ADR-0057), **frozen**
    /// here from the `RunStarted` payload — same immutability as
    /// [`RunState::harness`]: set once at create, never mutated (frozen for
    /// resume, ADR-0057 ¶4). `None` ⇒ the Run named no choice (every historical
    /// Run, and any Run created without one), so [`RunState::harness`] (the
    /// legacy signal) still decides the Run tier. `Some(Profile | Custom)` wins
    /// outright at this tier over `harness` — never merges — and also supplies
    /// model/effort to infra sessions (#563 AC18, amending ADR-0046's
    /// Run-only-harness rule for infra).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_choice: Option<crate::agent_choice::AgentChoice>,
    /// The Run tier of the **skills** selection (#669, ADR-0062), **frozen** here
    /// from the `RunStarted` payload — set once at create (a fired Run copies its
    /// Trigger's list), never mutated. Read at every node spawn as the `run` slot
    /// of `skill_selection::resolve`. Empty ⇒ the Run adds none (every
    /// historical Run: the payload omits the key).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) skills: Vec<crate::skill_selection::SkillRef>,
    /// The Run's `auto_fail` preference (ADR-0049), **frozen** here from the
    /// `RunStarted` payload — the **run** tier of
    /// [`crate::auto_fail::resolve_auto_fail`] (`node → Run → Projet →
    /// instance`). `None` ⇒ the Run stated no preference (every historical Run,
    /// and any Run created without an explicit choice — the payload omits the
    /// key), so a node `pdo fail` resolves through the project/instance tiers.
    /// Immutable, like [`RunState::harness`]: set once at create, never mutated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fail: Option<bool>,
    /// Provenance: the id of the Trigger that created this Run, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
    /// The library pipeline id this Run was created from (#377). Written to the
    /// `RunStarted` payload going forward; consumers (aggregated "by pipeline"
    /// stats) fall back to `pipeline_name` when it is absent — historical runs,
    /// bare-API/multipart creates, retries — so grouping survives a rename
    /// (#230). Additive and backward-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_id: Option<String>,
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
    /// Estimated USD cost for the Run (issue #272). `None` when no Claude Code
    /// transcript dir is found (UI renders "—"). Derived on read, never
    /// persisted; see [`CostStat`]. More durable than `loc`: archival leaves
    /// `~/.claude/projects/` intact, so an archived Run keeps its cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostStat>,
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
            failure_reason: None,
            awaiting_reason: None,
            awaiting_reason_code: None,
            nodes: HashMap::new(),
            edges: Vec::new(),
            node_defs: Vec::new(),
            provisioning_rules: Vec::new(),
            start_node: None,
            end_node: None,
            merge_resolver: None,
            loop_states: HashMap::new(),
            foreach_states: HashMap::new(),
            collection_states: HashMap::new(),
            switch_states: HashMap::new(),
            region_max_iter_overrides: HashMap::new(),
            forced_routes: HashMap::new(),
            target_repo: None,
            target_repos: Vec::new(),
            sandbox: SandboxMode::Off,
            sandbox_entries: None,
            sandbox_entries_raw_error: None,
            sandbox_env: None,
            sandbox_env_raw_error: None,
            sandbox_image: None,
            sandbox_image_raw_error: None,
            sandbox_prep: None,
            source_branch: None,
            fork_sha: None,
            harness: None,
            agent_choice: None,
            skills: Vec::new(),
            auto_fail: None,
            triggered_by: None,
            pipeline_id: None,
            sessions_spawned: 0,
            loc: None,
            cost: None,
        }
    }

    /// Status of `node_id` in this run's projection, if the node exists.
    ///
    /// Borrows (`NodeStatus` is `Clone`-not-`Copy`); use for status-only reads
    /// where the whole [`NodeState`] is not needed.
    pub fn node_status(&self, node_id: &str) -> Option<&NodeStatus> {
        self.nodes.get(node_id).map(|n| &n.status)
    }

    /// The latest **settled-complete** iteration of `node_id`, if any (#210).
    ///
    /// History-max over `Completed`/`Skipped` iterations (failed/stopped iters are
    /// quarantined — their artifacts stay on disk but are never resolvable as
    /// inputs), falling back to the head `iter` when the head status is settled
    /// but no per-iteration history exists (legacy states). This is the single
    /// home for the rule formerly duplicated as a free fn in `input_resolution`.
    ///
    /// `Skipped` counts (#620): the auto-skip writes an empty output precisely so a
    /// downstream resolver finds a concrete artifact, so a pruned producer's lap is
    /// resolvable exactly as it was when a skip projected `Completed`.
    pub fn latest_completed_iter(&self, node_id: &str) -> Option<i64> {
        let node = self.nodes.get(node_id)?;
        let from_history = node
            .iterations
            .iter()
            .filter(|it| it.status.is_settled_complete())
            .map(|it| it.iter)
            .max();
        from_history.or_else(|| node.status.is_settled_complete().then_some(node.iter))
    }

    /// All `Completed` iterations of `node_id`, ascending (#353).
    ///
    /// The set-valued twin of [`latest_completed_iter`], for `repeated`/pooled
    /// inputs that accumulate one artifact per completed lap. Failed/Stopped/
    /// Stale iters are quarantined (their artifacts stay on disk but are never
    /// resolvable as inputs). Falls back to `vec![head_iter]` when the head
    /// status is `Completed` but no per-iteration history exists (legacy
    /// states). Empty when the node has no completed iteration or does not
    /// exist.
    ///
    /// `NodeState.iterations` is already sorted by `iter` and deduplicated by
    /// `(node, iter)` in [`project`], so the history filter yields an ascending,
    /// duplicate-free `Vec` — no `BTreeSet` needed.
    ///
    /// Invariant: `completed_iters(n).last().copied() == latest_completed_iter(n)`.
    /// `latest_completed_iter` is NOT reimplemented on top of this: the spawn
    /// path is hot and calls it per resolution, so it avoids the `Vec` alloc.
    pub fn completed_iters(&self, node_id: &str) -> Vec<i64> {
        let Some(node) = self.nodes.get(node_id) else {
            return Vec::new();
        };
        let from_history: Vec<i64> = node
            .iterations
            .iter()
            .filter(|it| it.status.is_settled_complete())
            .map(|it| it.iter)
            .collect();
        if !from_history.is_empty() {
            return from_history;
        }
        if node.status.is_settled_complete() {
            vec![node.iter]
        } else {
            Vec::new()
        }
    }

    /// True iff `node_ids` is non-empty AND every id resolves to a node that is a
    /// **settled completion** — `Completed` or `Skipped`
    /// ([`NodeStatus::is_settled_complete`]).
    ///
    /// `Skipped` counts (#620): an auto-skipped node discharged its obligation with
    /// an empty output, so a run whose only "unfinished" node is a pruned one MUST
    /// still be able to reach `RunCompleted` — leaving it out would hang the run
    /// forever. The error-ish terminals (`Failed`/`Stopped`/`Stale`/`Interrupted`)
    /// do NOT count — those are owned by the stall / fail-fast / incident paths. A
    /// never-spawned id (no `NodeState`) counts as not-done. An empty set yields
    /// `false`, NOT vacuous-true: a run with no expected nodes is not "all done"
    /// (preserving the original `!is_empty()` guard).
    ///
    /// The authoritative node set is the caller's (`pipeline.nodes` at the
    /// completion/stall sites, the runtime `expected_node_ids` at the
    /// node-done sites) — `RunState` owns neither, so it receives the ids.
    pub fn all_nodes_completed(&self, node_ids: &[String]) -> bool {
        !node_ids.is_empty()
            && node_ids.iter().all(|id| {
                self.node_status(id)
                    .is_some_and(|s| s.is_settled_complete())
            })
    }

    /// Why this Run is **not schedulable yet** because its sandbox is still being
    /// prepared (#445) — `None` when a session may be launched.
    ///
    /// A sandboxed node's tmux window runs `docker exec … pdo-sbx-<run_id> …`. On a
    /// container that does not exist yet, `docker exec` exits 1 in ~30 ms, the window's
    /// command ends, the tmux session disappears, and ~25 s later the stale detector
    /// renders `session_died` — a failure that names tmux while the real fault is the
    /// ordering. The precondition therefore belongs to the *spawn*, not to the callers
    /// that reach it.
    ///
    /// | `sandbox` | `sandbox_prep` | verdict |
    /// |---|---|---|
    /// | `off` | (any) | `None` — the host path never grew a precondition |
    /// | profile | `Ready` | `None` — image + container + staging are guaranteed |
    /// | profile | `Pending` | blocked — the prep task is between its two events |
    /// | profile | `None` | blocked — the prep task has not reached its head event |
    ///
    /// The `None` arm blocks deliberately: `RunStarted` and `SandboxPrepStarted` are
    /// ~100 ms apart, and a read of `<run>/pipeline.yaml` inside that window wakes the
    /// watcher (inotify reports the *first read* of a fresh run dir as a modification).
    /// Blocking is also the fail-safe direction — the cost of a wrong `Some` is one
    /// deferred spawn replayed on `SandboxPrepReady`, the cost of a wrong `None` is a
    /// dead node.
    ///
    /// Pure: projected state in, decision out. The reason is the operator-facing
    /// sentence, so it names the profile (the *why this Run and not that one*).
    pub fn sandbox_spawn_block(&self) -> Option<String> {
        let profile = self.sandbox.profile()?;
        match self.sandbox_prep {
            Some(SandboxPrepState::Ready) => None,
            Some(SandboxPrepState::Pending) => Some(format!(
                "sandbox prep for run {} (profile `{profile}`) is still in progress: \
                 its container is not up, so no session can be launched yet",
                self.run_id
            )),
            None => Some(format!(
                "sandbox prep for run {} (profile `{profile}`) has not started yet: \
                 its container does not exist, so no session can be launched yet",
                self.run_id
            )),
        }
    }

    /// The ids of every **open** (not-`done`) region on this Run, across ALL
    /// region-state kinds (#601). Exhaustive by construction via
    /// [`RegionStateKind`]: an open region of any kind is the Pipeline Manager's
    /// domain, never a fail-fast run-level stall — so `run_stall_reason` can defer
    /// to a *total* predicate here instead of a hand-written per-map disjunction
    /// that a new region kind could silently escape (the #453 class, where each
    /// new region type reopened a frozen run reported `stalled = false`).
    pub(crate) fn open_region_ids(&self) -> Vec<String> {
        RegionStateKind::ALL
            .iter()
            .flat_map(|k| k.open_ids(self))
            .collect()
    }

    /// Whether any region of any kind is currently open (#601). See
    /// [`open_region_ids`](RunState::open_region_ids).
    pub(crate) fn has_open_region(&self) -> bool {
        !self.open_region_ids().is_empty()
    }
}

/// Every projected region-state map on a [`RunState`] that carries an "open"
/// (not-yet-`done`) lifecycle (#601). The point of this enum is a **compile-time
/// exhaustiveness anchor**: the `match` in [`RegionStateKind::open_ids`] has no
/// wildcard arm, so adding a new region-state map to `RunState` means adding a
/// variant here — and every consumer that iterates [`RegionStateKind::ALL`]
/// (notably `run_stall_reason`'s open-region defer) then covers it automatically.
/// Without it, a new region kind falls through to a silent `stalled = false`.
///
/// `switch_states` is deliberately **absent**: a switch is a routing *record*
/// (`SwitchState` has no `done` flag and no open/close lifecycle), not a region
/// that can hold a run open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionStateKind {
    Loop,
    ForEach,
    Collection,
}

impl RegionStateKind {
    /// Every variant. A new variant added to the enum must be added here too; the
    /// `region_state_kind_all_is_total` test asserts this list is complete, and
    /// [`open_ids`](Self::open_ids)'s wildcard-free `match` fails to compile until
    /// the variant is classified.
    pub(crate) const ALL: [RegionStateKind; 3] = [
        RegionStateKind::Loop,
        RegionStateKind::ForEach,
        RegionStateKind::Collection,
    ];

    /// The ids of the currently-open (not `done`) regions of this kind. Exhaustive
    /// `match`, NO wildcard — the compile-time anchor of #601.
    fn open_ids<'a>(self, run: &'a RunState) -> Box<dyn Iterator<Item = String> + 'a> {
        match self {
            RegionStateKind::Loop => Box::new(
                run.loop_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.loop_node_id.clone()),
            ),
            RegionStateKind::ForEach => Box::new(
                run.foreach_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.foreach_node_id.clone()),
            ),
            RegionStateKind::Collection => Box::new(
                run.collection_states
                    .values()
                    .filter(|s| !s.done)
                    .map(|s| s.region_id.clone()),
            ),
        }
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

pub(crate) fn project(events: &[Event]) -> Option<RunState> {
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
            | EventKind::RunReposEdited
            | EventKind::RunArchived
            | EventKind::RunInterrupted
            | EventKind::SandboxPrepStarted
            | EventKind::SandboxPrepReady => apply_run_event(&mut state, event),

            EventKind::NodeWaiting
            | EventKind::NodeStarted
            | EventKind::NodeCompleted
            | EventKind::NodeAutoCompleted
            | EventKind::NodeAwaitingUser
            | EventKind::NodeFailed
            | EventKind::NodeInterrupted
            | EventKind::NodeDelivered
            | EventKind::NodeStopped
            | EventKind::NodeStale
            | EventKind::NodeInvalidated
            | EventKind::FrontmatterRetryPending => apply_node_event(&mut state, event),

            EventKind::MergeConflictDetected
            | EventKind::MergeResolvedInNodeFavour
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
            EventKind::CollectionStarted
            | EventKind::CollectionEmpty
            | EventKind::CollectionDone => apply_collection_event(&mut state, event),

            EventKind::PipelineLint | EventKind::PipelineModified => {
                apply_pipeline_event(&mut state, event)
            }

            // Informational only: the node stays Running, no node/run state touched.
            EventKind::NodeBlockedOnLimit | EventKind::NodeAutoCompleteObserved => {}

            EventKind::CommandIssued => apply_command_event(&mut state, event),
        }
    }

    // #328: a log with no RunStarted is an invalid fragment (e.g. a late event
    // appended after a forget) — never surface it as a phantom Running run.
    state.started_at.as_ref()?;

    finalize(&mut state);

    Some(state)
}

/// The `reason` a non-green run terminal carries, if it says anything (#503).
///
/// An empty string is treated as absent: `Some("")` in the projection would make
/// the UI render an explanation box with nothing in it, which is worse than the
/// red dot it replaces.
fn run_event_reason(event: &Event) -> Option<String> {
    event
        .payload
        .as_ref()
        .and_then(|p| p.get("reason"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Build the payload of a park / give-up event carrying BOTH a machine reason
/// code and human prose (#601) — the state model's slug+prose contract, mirror
/// of a refusal body (ADR-0035: `error` slug + `message` prose). `code` is a
/// stable `snake_case` slug the manager/UI branch on; `reason` is the human
/// sentence (which, by convention, itself opens with `<code>: …` so a reader
/// with only the prose still sees the machine token). Used by every producer of
/// `RunInterrupted` so no non-advancement is prose-only on the wire.
pub(crate) fn interrupt_payload(code: &str, reason: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "reason_code": code, "reason": reason.into() })
}

/// Extract the machine slug a park/give-up prose reason is prefixed with (#601).
/// By convention every interrupt reason reads `"<slug>: <prose>"` (e.g.
/// `"session_died: tmux session … gone"`); the slug is the machine-branchable
/// code, the whole string stays the human sentence. Returns the slug when the
/// head before the first `": "` is a non-empty `snake_case` token (lowercase,
/// digits, `_`) and prose follows — else `None` (an un-prefixed legacy reason
/// carries no derivable code).
pub(crate) fn parse_reason_code(reason: &str) -> Option<String> {
    let (head, rest) = reason.split_once(": ")?;
    let looks_slug = !head.is_empty()
        && !rest.trim().is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    looks_slug.then(|| head.to_string())
}

/// The machine `reason_code` of a `RunInterrupted` event (#601): the explicit
/// `payload["reason_code"]` when present (authoritative), else parsed from the
/// prose reason's `<code>:` prefix (so historical logs written before the key
/// existed still surface a code). Mirrors [`run_event_reason`]'s emptiness
/// handling.
fn run_event_reason_code(event: &Event) -> Option<String> {
    let explicit = event
        .payload
        .as_ref()
        .and_then(|p| p.get("reason_code"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    explicit.or_else(|| {
        run_event_reason(event)
            .as_deref()
            .and_then(parse_reason_code)
    })
}

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
                    if let Some(raw) = payload.get("provisioning_rules") {
                        match serde_json::from_value::<Vec<crate::provisioning::ScopedRules>>(
                            raw.clone(),
                        ) {
                            Ok(rules) => state.provisioning_rules = rules,
                            Err(e) => warn!(
                                "run_started carries unreadable provisioning_rules ({raw}): {e}"
                            ),
                        }
                    }
                }
                if let Some(tr) = payload.get("target_repo").and_then(|v| v.as_str()) {
                    state.target_repo = Some(tr.to_string());
                }
                // #465 (ADR-0042): the secondary repos, frozen (repo/alias/sha) at
                // Run start. Absent → the mono-repo default (empty), which is what
                // every historical run and every mono-repo create carries. A present-
                // but-unreadable value degrades to empty with a `warn!` (LOUD, never a
                // panic — this runs before the transition guard): losing the read-only
                // context is a soft failure, not a reason to fail the Run replay.
                match payload.get("target_repos") {
                    None => {}
                    Some(raw) => match serde_json::from_value::<Vec<RepoPin>>(raw.clone()) {
                        Ok(pins) => state.target_repos = pins,
                        Err(e) => warn!(
                            "run_started carries an unreadable `target_repos` value \
                             ({raw}): {e}; projecting an empty secondary list (the Run \
                             runs mono-repo)."
                        ),
                    },
                }
                // #407: isolation mode, projected once and never mutated. Absent
                // (or malformed) → the `Off` default (host path). Never panics —
                // this applier runs before the transition guard.
                match payload.get("sandbox") {
                    None => {}
                    // #426/#432: the two pre-rename tokens are mapped to `Off` + a
                    // `warn!` BEFORE `parse` sees them — `parse` is syntactic now and
                    // would happily read `copy` as a profile name, turning a Run that
                    // predates the rename into a hard `RunFailed`. Only the payload
                    // arm keeps this compatibility; the tiers a user can still type
                    // (instance default, Trigger) fail loud by design (ADR-0031 §7).
                    Some(raw)
                        if raw
                            .as_str()
                            .is_some_and(|s| LEGACY_SANDBOX_TOKENS.contains(&s.trim())) =>
                    {
                        warn!(
                            "run_started carries the pre-#426 `sandbox` token ({raw}); \
                             projecting `off` (host path). `copy`/`pure` were renamed to \
                             `full`/`minimal` in #426, without alias."
                        )
                    }
                    Some(raw) => match serde_json::from_value::<SandboxMode>(raw.clone()) {
                        Ok(sandbox) => state.sandbox = sandbox,
                        // A blank / non-string value. The degradation goes toward LESS
                        // isolation (`Off` → host bash in `run-shell`, `cleanup_run`
                        // skipping `merge_back`/`teardown`, cost reading the wrong
                        // transcripts root). Silent is not an option: ADR-0030 pt 4
                        // forbids an unlogged host fallback.
                        Err(_) => warn!(
                            "run_started carries an unreadable `sandbox` value ({raw}); \
                             projecting `off` (host path)."
                        ),
                    },
                }
                // #432 (ADR-0031 §6): the sibling FROZEN entry list. Written together
                // with `sandbox` or not at all, so an absent key on a `sandbox` payload
                // can only mean "created by a pre-profiles daemon" — which the prep
                // resolves (virtual default) or fails (user profile), per its own
                // decision table. A present-but-unreadable value is kept verbatim in
                // `sandbox_entries_raw_error` so the prep can name it: re-resolving
                // would silently change what the already-spawned nodes saw.
                match payload.get("sandbox_entries") {
                    None => {}
                    Some(raw) => match serde_json::from_value::<Vec<String>>(raw.clone()) {
                        Ok(entries) => state.sandbox_entries = Some(entries),
                        Err(_) => {
                            state.sandbox_entries_raw_error = Some(raw.to_string());
                            warn!(
                                "run_started carries an unreadable `sandbox_entries` value \
                                 ({raw}); the sandbox prep will fail this Run loud rather \
                                 than re-resolve a different list."
                            );
                        }
                    },
                }
                // #468 (ADR-0031 §8): the sibling FROZEN env. Absent means "no profile
                // env" — indistinguishable from an empty map by construction, so unlike
                // `sandbox_entries` there is no legacy arm to re-resolve. A present-but-
                // unreadable value is kept verbatim so the prep can name it.
                //
                // The `warn!` names the KEY and the shape, never the values: a client token
                // in the systemd journal is an incident, and the journal outlives the Run.
                // `raw` here is the whole malformed value, which is why the arm below is the
                // one exception — it fires only on a payload that is NOT a string map, i.e.
                // one that cannot be carrying the user's values in the first place.
                match payload.get("sandbox_env") {
                    None => {}
                    Some(raw) => match serde_json::from_value::<
                        std::collections::BTreeMap<String, String>,
                    >(raw.clone())
                    {
                        Ok(env) => state.sandbox_env = Some(env),
                        Err(_) => {
                            state.sandbox_env_raw_error = Some(raw.to_string());
                            warn!(
                                "run_started carries an unreadable `sandbox_env` value \
                                 (not a map of strings); the sandbox prep will fail this Run \
                                 loud rather than pose a different environment than the \
                                 nodes that already launched saw."
                            );
                        }
                    },
                }
                // #467 (ADR-0031 §9): the third sibling, the FROZEN image source. Same shape as
                // the env — absent means "the profile posed none", so the instance-wide setting
                // decides and there is no legacy arm to re-resolve. A present-but-unreadable
                // value is kept verbatim so the prep can name it: degrading to "no image source"
                // would silently start the container in a DIFFERENT image than the nodes that
                // already launched ran in. The `warn!` may carry the raw value — an image ref is
                // not a secret, unlike an env value, and this arm only fires on a payload that is
                // not a valid image source in the first place.
                match payload.get("sandbox_image") {
                    None => {}
                    Some(raw) => {
                        match serde_json::from_value::<crate::sandbox_image::ProfileImage>(
                            raw.clone(),
                        ) {
                            Ok(image) => state.sandbox_image = Some(image),
                            Err(e) => {
                                state.sandbox_image_raw_error = Some(raw.to_string());
                                warn!(
                                    "run_started carries an unreadable `sandbox_image` value \
                                     ({raw}): {e}; the sandbox prep will fail this Run loud \
                                     rather than start it in a different image than the nodes \
                                     that already launched ran in."
                                );
                            }
                        }
                    }
                }
                if let Some(sb) = payload.get("source_branch").and_then(|v| v.as_str()) {
                    state.source_branch = Some(sb.to_string());
                }
                // #417: the FROZEN fork point, projected the same way as `source_branch`.
                // The create chokepoint writes this key for every new Run (a resolvable
                // `source_ref`); an absent key (every pre-#417 Run) leaves `None`, and the
                // LOC/diff base then falls back to `source_branch`, then `HEAD`.
                if let Some(fs) = payload.get("fork_sha").and_then(|v| v.as_str()) {
                    state.fork_sha = Some(fs.to_string());
                }
                // #551 (ADR-0046): the FROZEN Run harness, projected the same way as
                // `source_branch`. The create chokepoint only writes this key for a
                // non-empty choice, so an absent key (every historical Run, every Run
                // with no explicit harness) leaves `None` — the free nodes then resolve
                // through the instance default and the `claude` floor. A blank string
                // never reaches here (normalised away at the freeze), so it cannot win a
                // tier (#347).
                if let Some(h) = payload.get("harness").and_then(|v| v.as_str()) {
                    if !h.is_empty() {
                        state.harness = Some(h.to_string());
                    }
                }
                // #563 (ADR-0057): the FROZEN Run `AgentChoice`, projected the same
                // way as `harness` — the create chokepoint only writes this key for
                // an explicit (non-`Inherit`) choice, so an absent key (every
                // historical Run, every Run with no explicit choice) leaves `None`
                // and the legacy `harness` above still decides the Run tier.
                if let Some(v) = payload.get("agent_choice") {
                    if let Ok(choice) =
                        serde_json::from_value::<crate::agent_choice::AgentChoice>(v.clone())
                    {
                        state.agent_choice = Some(choice);
                    }
                }
                // #669 (ADR-0062): the FROZEN Run-tier skills, written by the create
                // chokepoint only when non-empty; absent ⇒ none (historical Runs).
                if let Some(v) = payload.get("skills") {
                    if let Ok(skills) =
                        serde_json::from_value::<Vec<crate::skill_selection::SkillRef>>(v.clone())
                    {
                        state.skills = skills;
                    }
                }
                // ADR-0049: the Run's frozen `auto_fail` tier. Absent key ⇒ `None`
                // (the Run stated no preference — every historical Run).
                if let Some(af) = payload.get("auto_fail").and_then(|v| v.as_bool()) {
                    state.auto_fail = Some(af);
                }
                if let Some(tb) = payload.get("triggered_by").and_then(|v| v.as_str()) {
                    state.triggered_by = Some(tb.to_string());
                }
                if let Some(pid) = payload.get("pipeline_id").and_then(|v| v.as_str()) {
                    state.pipeline_id = Some(pid.to_string());
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
            state.failure_reason = run_event_reason(event);
        }
        EventKind::RunSkipped => {
            // Graceful no-op (#245): terminal, like RunFailed/RunCompleted.
            // The run reached no `end` node (the selector short-circuited),
            // so end-node ports stay pending — only the run status reflects
            // "fired but nothing to do".
            state.status = RunStatus::Skipped;
            state.completed_at = Some(event.ts.clone());
            state.failure_reason = run_event_reason(event);
        }
        EventKind::RunHalted => {
            state.status = RunStatus::Halted;
            state.completed_at = Some(event.ts.clone());
            // A halt carries `message`, not `reason`. Surface it on the run too, so
            // all three non-green terminals answer "why?" through one field.
            state.failure_reason = event
                .payload
                .as_ref()
                .and_then(|p| p.get("message").or_else(|| p.get("reason")))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
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
            // A Run being driven again must not still display last time's cause
            // (#503) — same rule `NodeStarted` applies to `NodeState::failure_reason`.
            state.failure_reason = None;
            // An incident park (ADR-0049) clears the same way: a resumed Run is
            // no longer waiting on that incident.
            state.awaiting_reason = None;
            state.awaiting_reason_code = None;
        }
        // The runtime gave up on driving the run forward for a non-deliberate
        // reason (ADR-0049): park it `AwaitingUser` with the reason, NEVER
        // `Failed`, and never set `completed_at` (it is not terminal). Inert on
        // an already-terminal run (#221): an active give-up racing a genuine
        // completion must not un-terminalize it. A live run (Running /
        // AwaitingUser / Paused) parks; a Paused run parks too so an operator
        // sees why it will not resume clean.
        EventKind::RunInterrupted => {
            if state.status.is_live() {
                state.status = RunStatus::AwaitingUser;
                state.awaiting_reason = run_event_reason(event);
                state.awaiting_reason_code = run_event_reason_code(event);
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
        EventKind::RunReposEdited => {
            // #221 (double guard, reducer half): a terminal Run keeps the list it
            // was frozen with — a metadata event appended after a terminal event
            // (hand-crafted, replayed, or racing a completion) must NEVER re-open it
            // nor mutate its context. The handler already refuses the edit with a
            // 409, but the reducer is inert on its own so the log stays the single
            // source of truth even if an edit slipped in around the terminal event.
            if state.status.is_terminal() {
                return;
            }
            // The payload carries the whole re-frozen active list under
            // `target_repos` (mirror of the `RunStarted` arm above): overwrite
            // wholesale. Absent/unreadable degrades LOUDLY to "keep the previous
            // list" — never a panic (this applier runs inside `append_event`, before
            // the transition guard), and never a reset that would silently starve a
            // node reading a snapshot the projection forgot.
            if let Some(ref payload) = event.payload {
                match payload.get("target_repos") {
                    None => {}
                    Some(raw) => match serde_json::from_value::<Vec<RepoPin>>(raw.clone()) {
                        Ok(pins) => state.target_repos = pins,
                        Err(e) => warn!(
                            "run_repos_edited carries an unreadable `target_repos` value \
                             ({raw}): {e}; keeping the previous secondary list."
                        ),
                    },
                }
            }
        }
        EventKind::RunArchived => {
            state.status = RunStatus::Archived;
            state.start_node = None;
            state.end_node = None;
        }
        // #410: additive image-prep visibility. Non-terminal — `status` untouched,
        // only `sandbox_prep` moves. Emitted only for a sandboxed create (`full`/`minimal`);
        // `off`/historical runs never carry these, so the field stays `None`.
        EventKind::SandboxPrepStarted => {
            state.sandbox_prep = Some(SandboxPrepState::Pending);
        }
        EventKind::SandboxPrepReady => {
            state.sandbox_prep = Some(SandboxPrepState::Ready);
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
                        harness: None,
                        isolated_worktree: None,
                        skills: None,
                        missing_skills: Vec::new(),
                        skipped_skills: Vec::new(),
                        cost: None,
                        node_id: node_id.clone(),
                        status: NodeStatus::Waiting,
                        iter,
                        started_at: None,
                        completed_at: None,
                        failure_reason: None,
                        skip_reason: None,
                        iterations: Vec::new(),
                        frontmatter_retries: 0,
                        frontmatter_violations: Vec::new(),
                        missing_outputs: Vec::new(),
                        delivery: None,
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
                        harness: None,
                        isolated_worktree: None,
                        skills: None,
                        missing_skills: Vec::new(),
                        skipped_skills: Vec::new(),
                        cost: None,
                        node_id: node_id.clone(),
                        status: NodeStatus::Running,
                        iter,
                        started_at: Some(event.ts.clone()),
                        completed_at: None,
                        failure_reason: None,
                        skip_reason: None,
                        iterations: Vec::new(),
                        frontmatter_retries: 0,
                        frontmatter_violations: Vec::new(),
                        missing_outputs: Vec::new(),
                        delivery: None,
                    });
                node.status = NodeStatus::Running;
                node.iter = iter;
                node.started_at = Some(event.ts.clone());
                node.completed_at = None;
                node.failure_reason = None;
                // Reset the evidence vectors too, not just `completed_at` /
                // `failure_reason`: otherwise a *successful* retry leaves stale
                // violations on a green node.
                node.frontmatter_violations = Vec::new();
                node.missing_outputs = Vec::new();
                // #616/ADR-0046: freeze the harness this session ran on, from the
                // `NodeStarted` payload (node_spawn records the resolved harness
                // there). A re-spawn (retry / resume) re-freezes it; absent payload
                // (a pre-#616 event) leaves it `None`.
                if let Some(h) = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("harness"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    node.harness = Some(h.to_string());
                }
                // #653/ADR-0060: freeze where this NodeRun works, the same way.
                // A re-spawn of the same iteration re-poses the frozen value
                // rather than re-reading the (possibly edited) document, so the
                // recovery path lands back in the working directory it left.
                if let Some(isolated) = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("isolated_worktree"))
                    .and_then(|v| v.as_bool())
                {
                    node.isolated_worktree = Some(isolated);
                }
                // #669/ADR-0062: freeze the skills effectifs this session received,
                // and the ids it was promised but the bank no longer had. A re-spawn
                // re-freezes; a `script` node / pre-#669 event leaves `None`.
                if let Some(skills) = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("skills"))
                    .filter(|v| !v.is_null())
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                {
                    node.skills = Some(skills);
                }
                node.missing_skills = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("missing_skills"))
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                // #672: what the delivery could not write into the worktree.
                node.skipped_skills = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("skipped_skills"))
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                upsert_iteration(&mut node.iterations, iteration);
            }
        }
        EventKind::NodeCompleted | EventKind::NodeAutoCompleted => {
            if let Some(ref node_id) = event.node_id {
                // #600: a **skip** (`skip_node` / reachability auto-skip) can
                // complete a node that never started — that is the whole point of
                // skipping a node stuck waiting on an input that never came. Create
                // it directly as terminal, with no transient session-less `Running`
                // window for the liveness sweep to flag; it then counts as satisfied
                // for re-projection (a reopen never re-spawns it, FP #4).
                let is_skip = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("skipped"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // #620: the discriminator is "never ran", NOT the skip source. A
                // reachability auto-skip prunes a never-started node (→ `Skipped`,
                // greyed with its reason); a graceful `skip_node` skips a node that
                // DID start and reach a decision (→ stays `Completed`, and the run,
                // not the node, carries the "nothing to do" signal).
                let never_started_skip = is_skip && !state.nodes.contains_key(node_id);
                let done_status = if never_started_skip {
                    NodeStatus::Skipped
                } else {
                    NodeStatus::Completed
                };
                let skip_reason = if never_started_skip {
                    event
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("reason"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                } else {
                    None
                };
                if never_started_skip {
                    let iter = event.iter.unwrap_or(1);
                    state.nodes.insert(
                        node_id.clone(),
                        NodeState {
                            harness: None,
                            isolated_worktree: None,
                            skills: None,
                            missing_skills: Vec::new(),
                            skipped_skills: Vec::new(),
                            cost: None,
                            node_id: node_id.clone(),
                            status: done_status.clone(),
                            iter,
                            started_at: Some(event.ts.clone()),
                            completed_at: Some(event.ts.clone()),
                            failure_reason: None,
                            skip_reason: skip_reason.clone(),
                            iterations: vec![IterationInfo {
                                iter,
                                status: done_status.clone(),
                                started_at: Some(event.ts.clone()),
                                completed_at: Some(event.ts.clone()),
                            }],
                            frontmatter_retries: 0,
                            frontmatter_violations: Vec::new(),
                            missing_outputs: Vec::new(),
                            delivery: None,
                        },
                    );
                }
                if let Some(node) = state.nodes.get_mut(node_id) {
                    node.status = done_status.clone();
                    node.completed_at = Some(event.ts.clone());
                    if never_started_skip {
                        node.skip_reason = skip_reason.clone();
                    }
                    let iter = event.iter.unwrap_or(node.iter);
                    if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                        it.status = done_status.clone();
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
                            // Read BOTH shapes of the validation evidence: the
                            // after-retry branch puts `violations` at the top level,
                            // the `script` fail-fast branch nests them under `detail`.
                            // Don't flatten the producer — the nesting is what keeps a
                            // fail-fast audit trail distinguishable from an after-retry
                            // one (ADR-0035 §5).
                            //
                            // Collision-checked: the only other payload carrying
                            // `detail` is `MergeConflictDetected`, which routes to
                            // `apply_merge_event` and never reaches this arm.
                            let detail = payload.get("detail");
                            if let Some(arr) = payload
                                .get("violations")
                                .or_else(|| detail.and_then(|d| d.get("violations")))
                                .and_then(|v| v.as_array())
                            {
                                node.frontmatter_violations = arr.clone();
                            }
                            if let Some(arr) = payload
                                .get("missing")
                                .or_else(|| detail.and_then(|d| d.get("missing")))
                                .and_then(|v| v.as_array())
                            {
                                node.missing_outputs = arr
                                    .iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
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
        EventKind::NodeInterrupted => {
            // An infra incident (ADR-0049). Unlike `NodeFailed` this must be
            // visible even when the spawn aborted BEFORE `NodeStarted` opened an
            // iteration (ADR-0050 §1: a spawn abort names its node), so the node
            // is materialised if absent — with no iteration row, which is the
            // honest shape of "the session never started".
            if let Some(ref node_id) = event.node_id {
                let iter = event.iter.unwrap_or(1);
                let reason = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("reason"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let node = state
                    .nodes
                    .entry(node_id.clone())
                    .or_insert_with(|| NodeState {
                        harness: None,
                        isolated_worktree: None,
                        skills: None,
                        missing_skills: Vec::new(),
                        skipped_skills: Vec::new(),
                        cost: None,
                        node_id: node_id.clone(),
                        status: NodeStatus::Interrupted,
                        iter,
                        started_at: None,
                        completed_at: None,
                        failure_reason: None,
                        skip_reason: None,
                        iterations: Vec::new(),
                        frontmatter_retries: 0,
                        frontmatter_violations: Vec::new(),
                        missing_outputs: Vec::new(),
                        delivery: None,
                    });
                // Node-level status derives from the LATEST iteration, mirroring
                // the `NodeFailed` #196/#212 guard: interrupting an older iter
                // must not mislabel a node whose newer iteration is still live.
                if iter >= node.iter {
                    node.status = NodeStatus::Interrupted;
                    node.iter = iter;
                    node.failure_reason = reason;
                    // Not a green completion: leave `completed_at` untouched.
                    // Carry the same validation evidence a `NodeFailed` would
                    // (#490): an output-validation interrupt should render the red
                    // banner with the missing ports / frontmatter violations, not
                    // an empty list. Both shapes read, exactly like `NodeFailed`.
                    if let Some(ref payload) = event.payload {
                        let detail = payload.get("detail");
                        if let Some(arr) = payload
                            .get("violations")
                            .or_else(|| detail.and_then(|d| d.get("violations")))
                            .and_then(|v| v.as_array())
                        {
                            node.frontmatter_violations = arr.clone();
                        }
                        if let Some(arr) = payload
                            .get("missing")
                            .or_else(|| detail.and_then(|d| d.get("missing")))
                            .and_then(|v| v.as_array())
                        {
                            node.missing_outputs = arr
                                .iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();
                        }
                    }
                }
                if let Some(it) = node.iterations.iter_mut().find(|i| i.iter == iter) {
                    it.status = NodeStatus::Interrupted;
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
        // #654 / ADR-0060: what the delivery put on the Run's branch. Read from
        // the payload rather than re-derived, because the tips are the only trace
        // of a non-isolated NodeRun's contribution — it owns no branch to diff.
        // The last delivery for the node wins: a re-run of the same node delivers
        // again, and the diff must show the latest one.
        EventKind::NodeDelivered => {
            if let (Some(node_id), Some(payload)) = (&event.node_id, &event.payload) {
                if let Some(node) = state.nodes.get_mut(node_id) {
                    if let (Some(before), Some(after)) = (
                        payload.get("before").and_then(|v| v.as_str()),
                        payload.get("after").and_then(|v| v.as_str()),
                    ) {
                        node.delivery = Some(NodeDelivery {
                            before: before.to_string(),
                            after: after.to_string(),
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

/// `SwitchRouted`: a switch node both records its chosen branch in
/// `switch_states` AND writes a synthetic `Completed` node entry (the switch has
/// no NodeRun session of its own), so it is kept as its own concern rather than
/// folded into the node applier.
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
                    harness: None,
                    isolated_worktree: None,
                    skills: None,
                    missing_skills: Vec::new(),
                    skipped_skills: Vec::new(),
                    cost: None,
                    node_id: node_id.to_string(),
                    status: NodeStatus::Completed,
                    iter,
                    started_at: Some(event.ts.clone()),
                    completed_at: Some(event.ts.clone()),
                    failure_reason: None,
                    skip_reason: None,
                    iterations: Vec::new(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                    missing_outputs: Vec::new(),
                    delivery: None,
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

/// Collection-region barrier accounting (ADR-0011 / #269): track the resolved
/// collection size and the done flag, keyed by region id. The region twin of
/// [`apply_foreach_event`]; an empty collection short-circuits straight to
/// done. Panic-free on malformed payloads (missing keys are ignored).
fn apply_collection_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::CollectionStarted => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    let total_items = payload
                        .get("total_items")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let entry = payload
                        .get("entry")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    // Pre-#453 payloads carry `entry` but no `members`: fall
                    // back to the entry alone, which is the whole region for the
                    // common single-member collection.
                    let members: Vec<String> = payload
                        .get("members")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_else(|| {
                            if entry.is_empty() {
                                Vec::new()
                            } else {
                                vec![entry.clone()]
                            }
                        });
                    state
                        .collection_states
                        .entry(region_id.to_string())
                        .or_insert_with(|| CollectionState {
                            region_id: region_id.to_string(),
                            total_items,
                            done: false,
                            entry,
                            members,
                        });
                }
            }
        }
        EventKind::CollectionEmpty => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    let cs = state
                        .collection_states
                        .entry(region_id.to_string())
                        .or_insert_with(|| CollectionState {
                            region_id: region_id.to_string(),
                            total_items: 0,
                            done: false,
                            entry: String::new(),
                            members: Vec::new(),
                        });
                    cs.done = true;
                }
            }
        }
        EventKind::CollectionDone => {
            if let Some(ref payload) = event.payload {
                if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                    if let Some(cs) = state.collection_states.get_mut(region_id) {
                        cs.done = true;
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
            // Informational — the run either spawns a resolver, resolves the
            // conflict in the node's favour (#503) or fails.
        }
        EventKind::MergeResolvedInNodeFavour => {
            // Informational (#503) — the completion carries on from here, so the
            // node's own `NodeCompleted` moves the state. Nothing to project: this
            // event exists so that a rewritten pipeline branch is never silent.
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
            // Node_defs/edges are re-parsed from the file at scheduling time
            // (`spawn_ready_after_event`), so a live run picks up newly-added nodes
            // on the next tick with no status change here.
            //
            // Don't un-terminalize the run (#221): a `PipelineModified` is a passive
            // signal that a stray or foreign file write can emit, even for a node
            // outside this run's DAG. Reopening a terminal run here leaves it
            // phantom-`running` forever (no reliable re-completion path), holds its
            // manager session and worktree, makes overlap-`skip` triggers skip every
            // subsequent fire, and lets a later `resume_run` re-spawn satisfied loops.
            // Picking up newly-added work is the explicit `resume_run`, not a side
            // effect of the file watcher.
        }
        _ => {}
    }
}

/// `CommandIssued`: the projection-relevant manager/operator commands. A command
/// dispatcher by nature — `resume_run` re-opens a terminal run and `end_region`
/// closes a loop region — so the whole event is kept in one applier even though
/// it touches both run status and `loop_states`.
fn apply_command_event(state: &mut RunState, event: &Event) {
    if let Some(ref payload) = event.payload {
        let cmd = payload.get("command").and_then(|v| v.as_str());
        // Re-opening a terminal Run (ADR-0049 / ADR-0032 amended): `terminal ≠
        // locked`. A human gesture — the global `reopen_run` (Play button), or a
        // targeted command that embeds its own re-open — lifts ANY terminal Run
        // back to `Running` by a safe re-projection: the satisfied `(node, iter)`
        // stay `Completed` (the scheduler's dedup refuses to re-spawn them,
        // anti-#221), only the unsatisfied work runs. `resume_run` is the
        // historical name of the same gesture, kept so every replayed log still
        // re-opens (append-only). Both clear the previous terminal/incident
        // reason — a Run being driven again must not still show last time's
        // cause (#503) — while the terminal *label* stays in the event log.
        // `Completed`/`Skipped` are in the set on purpose: a finished Run can pick
        // up a newly-added node (FP #6).
        if matches!(cmd, Some("reopen_run") | Some("resume_run"))
            && matches!(
                state.status,
                RunStatus::Halted
                    | RunStatus::Failed
                    | RunStatus::Completed
                    | RunStatus::Skipped
                    | RunStatus::AwaitingUser
            )
        {
            state.status = RunStatus::Running;
            state.completed_at = None;
            state.failure_reason = None;
            state.awaiting_reason = None;
            state.awaiting_reason_code = None;
            // Re-drive the interrupted work (ADR-0049 default: restart with the
            // partial artefacts, which persist in the sub-worktree). Drop each
            // `Interrupted` node from the projection — exactly like
            // `NodeInvalidated` — so the scheduler re-spawns it fresh (its
            // sub-worktree is reused, feeding the partial work) instead of
            // leaving it parked; without this, [`finalize`] would re-derive
            // `AwaitingUser` from the still-interrupted node and the re-open
            // would not stick. The satisfied `Completed` nodes are untouched, so
            // the scheduler never re-spawns them (anti-#221). The event log keeps
            // every `NodeInterrupted` — this only rewinds the *projection*.
            state
                .nodes
                .retain(|_, n| n.status != NodeStatus::Interrupted);
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
        // #600 / ADR-0011: `set_region_max_iter` raises a bounded region's cap in
        // flight. Absolute and last-write-wins (unlike `bump_region`, which is
        // additive) — the operator names the total number of laps they want, not a
        // delta. The scheduler reads this override in place of the region's declared
        // `max_iter`, so it lifts a literal cap and a `$var` cap the same way. A
        // non-positive cap is ignored (a region is never made zero-lap by a stray
        // command); the source of truth stays the append-only log, so the raise
        // holds across a reopen re-projection.
        if cmd == Some("set_region_max_iter") {
            if let (Some(region_id), Some(n)) = (
                payload.get("region_id").and_then(|v| v.as_str()),
                payload.get("max_iter").and_then(|v| v.as_i64()),
            ) {
                if n > 0 {
                    state
                        .region_max_iter_overrides
                        .insert(region_id.to_string(), n);
                }
            }
        }
        // #600 / ADR-0011: `force_route` records an explicit exit from a node or a
        // region to a target, short-circuiting the source's `when:` edges. Folded
        // per source (last-write-wins) so the effect is deterministic on every
        // re-projection — the forced route is NOT re-decided by `when:` on the next
        // lap or after a reopen (FP #8). Both endpoints are validated against the
        // pipeline snapshot before the command is appended (the handler), so the
        // projection trusts the payload.
        if cmd == Some("force_route") {
            if let (Some(from), Some(target)) = (
                payload.get("from").and_then(|v| v.as_str()),
                payload.get("target").and_then(|v| v.as_str()),
            ) {
                state
                    .forced_routes
                    .insert(from.to_string(), target.to_string());
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
    for node in state.nodes.values_mut() {
        node.iterations.sort_by_key(|i| i.iter);
        if let Some(max_iter) = node.iterations.last() {
            node.iter = max_iter.iter;
        }
    }

    // Derive run-level awaiting_user from node states. Two causes, one status
    // (ADR-0049): an *interactive* node genuinely waiting on its user, OR an
    // *interrupted* node whose infra incident parked the run. Both lift a
    // `Running` run to `AwaitingUser`; only the incident carries an
    // `awaiting_reason`, which keeps the two distinguishable from the run state
    // alone.
    if state.status == RunStatus::Running
        && state
            .nodes
            .values()
            .any(|n| n.status == NodeStatus::AwaitingUser || n.status == NodeStatus::Interrupted)
    {
        state.status = RunStatus::AwaitingUser;
    }

    // Surface the incident reason when the park is caused by an interrupted node
    // and no run-level `RunInterrupted` already set one. An interactive-only
    // wait leaves `awaiting_reason` `None` (its prompt is the node's business,
    // not an incident). Deterministic pick — the lowest node id — so replay is
    // stable when several nodes interrupted.
    if state.status == RunStatus::AwaitingUser && state.awaiting_reason.is_none() {
        if let Some((_, node)) = state
            .nodes
            .iter()
            .filter(|(_, n)| n.status == NodeStatus::Interrupted)
            .min_by(|a, b| a.0.cmp(b.0))
        {
            state.awaiting_reason = node.failure_reason.clone();
            // #601: carry the machine slug too, derived from the node reason's
            // `<code>: <prose>` prefix (`session_died`, `spawn_aborted`,
            // `boot_recovery`, …). A node-level interrupt has no run-level
            // `reason_code` payload to read, so the prose prefix is the source.
            state.awaiting_reason_code = node.failure_reason.as_deref().and_then(parse_reason_code);
        }
    }
}

/// Is this Run *stalled* (#180)? A run with no node currently `running` or
/// `waiting` and no active merge resolver, where at least one node has gone
/// `stale`, has nothing left to drive it forward.
///
/// **VESTIGIAL since #469 (ADR-0032): reserved for historical Runs.** This
/// predicate tests `NodeStatus::Stale` and nothing else, and nothing in the
/// daemon produces that status any more — so it is constantly `false` for every
/// new Run, and **#180's amber dot will never light again**. That is a deliberate
/// removal of functionality, recorded in ADR-0032 § "Ce qu'on ne fait pas": do
/// not wire a producer back in to "fix" the amber dot. What replaced the stale
/// verdict is the absence of one — an agent alive but not progressing stays
/// `Running`, with its session attachable and the human's Stop/Retry to hand.
///
/// The function is kept, live and correct, because the event log is append-only:
/// a Run that recorded a `NodeStale` before #469 still projects `Stale` and must
/// still render as it always did.
pub(crate) fn is_stalled(run: &RunState) -> bool {
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

pub(crate) fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn generate_run_id() -> String {
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
pub(crate) struct RegionRoute {
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
pub(crate) fn collect_region_routes(events: &[Event]) -> HashMap<String, RegionRoute> {
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

pub(crate) fn collect_cycle_extensions(events: &[Event]) -> HashMap<String, i64> {
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

    fn full() -> SandboxMode {
        SandboxMode::Profile("full".into())
    }
    fn minimal() -> SandboxMode {
        SandboxMode::Profile("minimal".into())
    }

    #[test]
    fn explicit_off_beats_full_default() {
        // The bug #410 exists to fix: an explicit `off` must survive a `full`/`minimal`
        // instance default — otherwise the default could never be overridden downward.
        assert_eq!(
            effective_sandbox(Some(SandboxMode::Off), None, full()),
            SandboxMode::Off
        );
    }

    #[test]
    fn explicit_wins_over_trigger_and_default() {
        assert_eq!(
            effective_sandbox(Some(minimal()), Some(full()), SandboxMode::Off),
            minimal()
        );
    }

    #[test]
    fn trigger_used_when_no_explicit() {
        assert_eq!(
            effective_sandbox(None, Some(minimal()), SandboxMode::Off),
            minimal()
        );
        // A trigger's explicit `off` also stands over a `full` instance default.
        assert_eq!(
            effective_sandbox(None, Some(SandboxMode::Off), full()),
            SandboxMode::Off
        );
    }

    #[test]
    fn all_none_falls_to_instance_default() {
        assert_eq!(effective_sandbox(None, None, full()), full());
        assert_eq!(
            effective_sandbox(None, None, SandboxMode::DEFAULT),
            SandboxMode::Off
        );
    }

    #[test]
    fn default_sandbox_with_precedence() {
        // stored valid wins.
        assert_eq!(default_sandbox_with(Some("minimal".into())), minimal());
        assert_eq!(default_sandbox_with(Some("full".into())), full());
        // empty sentinel → unset → default (Off) (env not set in this harness).
        assert_eq!(default_sandbox_with(Some(String::new())), SandboxMode::Off);
        // absent → default.
        assert_eq!(default_sandbox_with(None), SandboxMode::Off);
    }

    /// #432, the behaviour change worth pinning: an unrecognised stored token is NO
    /// LONGER demoted to `off`. It is a PROFILE NAME, it wins the tier, and the
    /// create-run chokepoint 400s on it by name (ADR-0031 §7 — never a silent fallback
    /// toward less isolation). `default_sandbox_with` is pure and cannot know whether
    /// the profile exists; that is the edge's job.
    #[test]
    fn a_stored_unknown_name_stays_the_winning_tier() {
        assert_eq!(
            default_sandbox_with(Some("full-no-mcp".into())),
            SandboxMode::Profile("full-no-mcp".into())
        );
        // Even the two pre-#426 tokens: they are just names now, and they will fail loud
        // at launch instead of quietly demoting the whole instance to the host path.
        assert_eq!(
            default_sandbox_with(Some("copy".into())),
            SandboxMode::Profile("copy".into())
        );
    }

    #[test]
    fn sandbox_mode_parse_and_as_str_round_trip() {
        for mode in [SandboxMode::Off, full(), minimal()] {
            assert_eq!(SandboxMode::parse(mode.as_str()), Some(mode));
        }
        // `off` is case / whitespace tolerant (a closed token nobody owns the spelling of).
        assert_eq!(SandboxMode::parse("  OFF "), Some(SandboxMode::Off));
        // A profile name is trimmed but NEVER lowercased — see the asymmetry documented on
        // `parse` and on `sandbox_profile::validate_profile_name`.
        assert_eq!(
            SandboxMode::parse("  Full-No-MCP "),
            Some(SandboxMode::Profile("Full-No-MCP".into()))
        );
        // Blank is the ONLY `None`: `parse` is syntactic since #432, existence is a
        // database question answered at the edge.
        assert_eq!(SandboxMode::parse(""), None);
        assert_eq!(SandboxMode::parse("   "), None);
        assert_eq!(
            SandboxMode::parse("nope"),
            Some(SandboxMode::Profile("nope".into()))
        );
    }

    /// The wire form is a BARE STRING, byte-identical to the pre-#432 enum for the two
    /// names that existed. Every historical `RunStarted` payload must round-trip
    /// unchanged, which is what makes this a type change and not a data migration.
    #[test]
    fn sandbox_mode_serialises_as_a_bare_string() {
        for wire in ["off", "full", "minimal", "full-no-mcp"] {
            let parsed = SandboxMode::parse(wire).unwrap();
            let json = serde_json::to_value(&parsed).unwrap();
            assert_eq!(json, serde_json::json!(wire), "{wire} must round-trip");
            assert_eq!(
                serde_json::from_value::<SandboxMode>(json).unwrap(),
                parsed,
                "{wire} must deserialise back"
            );
        }
        // A blank / non-string value fails deserialisation rather than becoming `Off`.
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!("")).is_err());
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!(null)).is_err());
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!(3)).is_err());
    }

    /// #426: a pre-rename `copy`/`pure` in a persisted `RunStarted` degrades to
    /// `Off` — the host path. The degradation is DELIBERATE (no alias, ADR-0031 §1)
    /// but it goes toward LESS isolation, so `apply_run_event` logs it. This test
    /// pins the choice so nobody "fixes" it into an alias by accident.
    ///
    /// #432 makes it load-bearing in a NEW way: `parse` would now happily read `copy` as
    /// a profile name, so without the explicit legacy-token arm a Run that predates the
    /// rename would `RunFailed` at its next boot recovery instead of running on the host.
    #[test]
    fn run_started_with_a_pre_rename_sandbox_token_projects_off() {
        for token in ["copy", "pure"] {
            let events = vec![make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p", "sandbox": token }),
            )];
            let state = project(&events).unwrap();
            assert_eq!(state.sandbox, SandboxMode::Off, "token {token}");
            assert_eq!(state.sandbox_prep, None);
            assert_eq!(state.sandbox_entries, None);
        }
    }

    /// The `Option` on `sandbox_entries` is load-bearing: `Some(vec![])` is a legitimate
    /// resolution — it IS `minimal` — while `None` means "no key, a pre-profiles
    /// payload". Confusing the two would send `minimal` Runs down the re-resolve arm.
    #[test]
    fn an_empty_frozen_list_projects_as_some_not_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "minimal",
                "sandbox_entries": [],
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox, SandboxMode::Profile("minimal".into()));
        assert_eq!(state.sandbox_entries, Some(Vec::new()));
        assert_eq!(state.sandbox_entries_raw_error, None);
    }

    #[test]
    fn a_frozen_list_projects_verbatim() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "full-no-mcp",
                "sandbox_entries": [".claude/skills", ".gitconfig"],
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox, SandboxMode::Profile("full-no-mcp".into()));
        assert_eq!(
            state.sandbox_entries,
            Some(vec![".claude/skills".to_string(), ".gitconfig".to_string()])
        );
    }

    /// A legacy payload: `sandbox` present, no `sandbox_entries`. The prep's decision
    /// table then re-resolves a virtual default and fails a user profile — but the
    /// PROJECTION must simply report the absence, never invent a list.
    #[test]
    fn a_legacy_sandbox_payload_projects_no_frozen_list() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p", "sandbox": "full" }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox, SandboxMode::Profile("full".into()));
        assert_eq!(state.sandbox_entries, None);
        assert_eq!(state.sandbox_entries_raw_error, None);
    }

    /// A present-but-unreadable list keeps its RAW value, so the prep can name it in the
    /// failure reason. Silently re-resolving would change what the already-spawned nodes
    /// saw — the one thing the freeze exists to prevent.
    #[test]
    fn an_unreadable_frozen_list_is_kept_raw_for_the_failure_reason() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "full",
                "sandbox_entries": 42,
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox_entries, None);
        assert_eq!(state.sandbox_entries_raw_error.as_deref(), Some("42"));
    }

    /// `sandbox_entries` is INTERNAL to the projection when it is unreadable: the wire
    /// view of a Run must not grow a field nothing consumes.
    #[test]
    fn the_raw_error_is_never_serialised() {
        let mut state = RunState::new("r1".into(), "p".into());
        state.sandbox_entries_raw_error = Some("42".into());
        let value = serde_json::to_value(&state).unwrap();
        assert!(value.get("sandbox_entries_raw_error").is_none());
        // …and an absent list is skipped entirely (back-compat of the `off` shape).
        assert!(value.get("sandbox_entries").is_none());
    }

    #[test]
    fn a_frozen_env_projects_verbatim() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "chrome",
                "sandbox_entries": [".claude/skills"],
                "sandbox_env": { "PUPPETEER_EXECUTABLE_PATH": "/usr/bin/chromium", "FOO": "bar" },
            }),
        )];
        let state = project(&events).unwrap();
        let env = state.sandbox_env.expect("the frozen env must project");
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(
            env.get("PUPPETEER_EXECUTABLE_PATH").map(String::as_str),
            Some("/usr/bin/chromium")
        );
        assert_eq!(state.sandbox_env_raw_error, None);
    }

    /// The asymmetry with `sandbox_entries`, pinned: an absent `sandbox_env` is NOT a
    /// legacy arm to re-resolve. A pre-#468 daemon could pose no profile env at all, so
    /// absence and emptiness describe the same container — and the prep must treat both as
    /// "no env" rather than reading the live profile, which would violate the freeze by
    /// ADDING variables to a Run in flight.
    #[test]
    fn an_absent_frozen_env_projects_none_and_means_empty() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "full",
                "sandbox_entries": [".claude/skills"],
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox_env, None);
        assert_eq!(state.sandbox_env_raw_error, None);
    }

    /// A present-but-unreadable env keeps its RAW value so the prep can fail loud. The
    /// alternative — degrading to "no env" — would silently start the container without the
    /// `PUPPETEER_EXECUTABLE_PATH` its MCP servers need, and look like a plugin bug.
    #[test]
    fn an_unreadable_frozen_env_is_kept_raw_for_the_failure_reason() {
        for bad in [
            serde_json::json!(42),
            serde_json::json!([1, 2]),
            serde_json::json!({ "FOO": 3 }),
        ] {
            let events = vec![make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "p",
                    "sandbox": "full",
                    "sandbox_entries": [],
                    "sandbox_env": bad,
                }),
            )];
            let state = project(&events).unwrap();
            assert_eq!(state.sandbox_env, None, "{bad} must not project a map");
            assert!(
                state.sandbox_env_raw_error.is_some(),
                "{bad} must be kept raw for the failure reason"
            );
        }
    }

    /// The raw-error twin is internal, and an absent env is skipped on the wire — a
    /// historical Run's JSON is byte-identical to before #468.
    #[test]
    fn the_env_raw_error_is_never_serialised() {
        let mut state = RunState::new("r1".into(), "p".into());
        state.sandbox_env_raw_error = Some("42".into());
        let value = serde_json::to_value(&state).unwrap();
        assert!(value.get("sandbox_env_raw_error").is_none());
        assert!(value.get("sandbox_env").is_none());
    }

    #[test]
    fn a_frozen_image_source_projects_verbatim() {
        for (payload, expected) in [
            (
                serde_json::json!({ "kind": "registry", "ref": "ghcr.io/acme/agent:1.4" }),
                crate::sandbox_image::ProfileImage::Registry {
                    image_ref: "ghcr.io/acme/agent:1.4".to_string(),
                },
            ),
            (
                serde_json::json!({ "kind": "dockerfile", "path": "/repo/Dockerfile.chrome-dev" }),
                crate::sandbox_image::ProfileImage::Dockerfile {
                    path: "/repo/Dockerfile.chrome-dev".to_string(),
                },
            ),
        ] {
            let events = vec![make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "p",
                    "sandbox": "chrome",
                    "sandbox_entries": [".claude/skills"],
                    "sandbox_image": payload,
                }),
            )];
            let state = project(&events).unwrap();
            assert_eq!(state.sandbox_image, Some(expected));
            assert_eq!(state.sandbox_image_raw_error, None);
        }
    }

    /// The same asymmetry with `sandbox_entries` as the env has: an absent `sandbox_image` is NOT
    /// a legacy arm to re-resolve, it means "the profile posed none" — and the profile default of
    /// #471 then decides, exactly as the two retired settings did before it.
    #[test]
    fn an_absent_frozen_image_source_projects_none() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "full",
                "sandbox_entries": [".claude/skills"],
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox_image, None);
        assert_eq!(state.sandbox_image_raw_error, None);
    }

    /// A present-but-unreadable image source keeps its RAW value so the prep can fail loud.
    /// Degrading to "the instance setting decides" would silently start the container in a
    /// DIFFERENT image than the nodes that already launched ran in.
    #[test]
    fn an_unreadable_frozen_image_source_is_kept_raw_for_the_failure_reason() {
        for bad in [
            serde_json::json!(42),
            serde_json::json!("ghcr.io/acme/agent:1.4"),
            serde_json::json!({ "kind": "ecr", "ref": "x:1" }),
            // The right `kind`, the wrong field — a half-migrated payload must not resolve.
            serde_json::json!({ "kind": "registry", "path": "/x" }),
        ] {
            let events = vec![make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "p",
                    "sandbox": "full",
                    "sandbox_entries": [],
                    "sandbox_image": bad,
                }),
            )];
            let state = project(&events).unwrap();
            assert_eq!(state.sandbox_image, None, "{bad} must not project a source");
            assert!(
                state.sandbox_image_raw_error.is_some(),
                "{bad} must be kept raw for the failure reason"
            );
        }
    }

    /// The raw-error twin is internal, and an absent image source is skipped on the wire — a
    /// historical Run's JSON is byte-identical to before #467.
    #[test]
    fn the_image_source_raw_error_is_never_serialised() {
        let mut state = RunState::new("r1".into(), "p".into());
        state.sandbox_image_raw_error = Some("42".into());
        let value = serde_json::to_value(&state).unwrap();
        assert!(value.get("sandbox_image_raw_error").is_none());
        assert!(value.get("sandbox_image").is_none());
    }

    /// #471, the one guarantee that has nothing to do with API compatibility: an **archived** Run
    /// whose payload was written by an older daemon still opens. Payload projection is additive —
    /// keys it does not know are ignored, keys it knows are read — so a `run_started` carrying the
    /// retired setting names (a hand-edited payload, or a future daemon's key seen by an older
    /// reader) projects exactly like one without them, including the #467 frozen source.
    #[test]
    fn an_older_payload_with_retired_setting_keys_still_projects() {
        let events = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "sandbox": "chrome",
                "sandbox_entries": [".claude/skills"],
                "sandbox_image": { "kind": "registry", "ref": "ghcr.io/acme/agent:1.4" },
                // Names #471 retired from `instance_config`. They never belonged in a Run payload;
                // seeing them here must be a non-event, not a projection failure.
                "image_source": "dockerfile",
                "dockerfile_path": "/repo/docker/sbx.Dockerfile",
            }),
        )];
        let state = project(&events).unwrap();
        assert_eq!(state.pipeline_name, "p");
        assert_eq!(state.sandbox.as_str(), "chrome");
        assert_eq!(
            state.sandbox_entries.as_deref(),
            Some(&[".claude/skills".to_string()][..])
        );
        assert_eq!(
            state.sandbox_image,
            Some(crate::sandbox_image::ProfileImage::Registry {
                image_ref: "ghcr.io/acme/agent:1.4".to_string(),
            }),
            "the frozen source still reads: an unknown sibling key changes nothing"
        );
        assert_eq!(state.sandbox_image_raw_error, None);
        // And the projection does not echo the stray keys back onto the wire.
        let value = serde_json::to_value(&state).unwrap();
        assert!(value.get("image_source").is_none(), "{value}");
        assert!(value.get("dockerfile_path").is_none(), "{value}");
    }

    #[test]
    fn sandbox_prep_started_projects_pending_then_ready() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p", "sandbox": "minimal" }),
            ),
            make_event(EventKind::SandboxPrepStarted, None, None),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox_prep, Some(SandboxPrepState::Pending));
        // status untouched by the informational event.
        assert_eq!(state.status, RunStatus::Running);

        let mut ready = events;
        ready.push(make_event(EventKind::SandboxPrepReady, None, None));
        let state = project(&ready).unwrap();
        assert_eq!(state.sandbox_prep, Some(SandboxPrepState::Ready));
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn off_run_never_carries_sandbox_prep() {
        // Byte-identical `off` invariant: no prep events, field stays None (and is
        // skipped from serialization).
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::NodeStarted, Some("n1"), Some(1)),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.sandbox_prep, None);
        let value = serde_json::to_value(&state).unwrap();
        assert!(value.get("sandbox_prep").is_none());
    }

    #[test]
    fn node_started_freezes_the_harness_onto_the_node_state() {
        // #616/ADR-0046: the Run view shows, per node, the harness its session was
        // frozen on — read from the `NodeStarted` payload into `NodeState::harness`.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event_with_payload(
                EventKind::NodeStarted,
                Some("impl"),
                serde_json::json!({ "harness": "copilot" }),
            ),
            make_event_with_payload(
                EventKind::NodeStarted,
                Some("review"),
                serde_json::json!({ "harness": "claude" }),
            ),
        ];
        let state = project(&events).unwrap();
        assert_eq!(
            state.nodes["impl"].harness.as_deref(),
            Some("copilot"),
            "the node's frozen harness must project"
        );
        assert_eq!(state.nodes["review"].harness.as_deref(), Some("claude"));

        // A node that never opened a `NodeStarted` carries no harness, and the field
        // is skipped from the wire (byte-identical to a pre-#616 snapshot).
        let value = serde_json::to_value(&state.nodes["impl"]).unwrap();
        assert_eq!(value["harness"], "copilot");
        let bare = super::NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            harness: None,
            isolated_worktree: None,
            cost: None,
            node_id: "x".into(),
            status: NodeStatus::Waiting,
            iter: 1,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: vec![],
            frontmatter_retries: 0,
            frontmatter_violations: vec![],
            missing_outputs: vec![],
            delivery: None,
        };
        assert!(
            serde_json::to_value(&bare)
                .unwrap()
                .get("harness")
                .is_none(),
            "an absent harness is skipped from the wire"
        );
    }

    #[test]
    fn sandbox_spawn_block_gates_on_the_projected_prep_state() {
        // The whole decision table of `sandbox_spawn_block`, which is what stands
        // between the pipeline watcher and a `docker exec` into a container that does
        // not exist yet.
        let sandboxed = |prep: Option<SandboxPrepState>| {
            let mut s = RunState::new("r1".into(), "p".into());
            s.sandbox = SandboxMode::Profile("full".into());
            s.sandbox_prep = prep;
            s
        };

        // Prep finished: the container is up, spawning is legal.
        assert!(sandboxed(Some(SandboxPrepState::Ready))
            .sandbox_spawn_block()
            .is_none());

        // Prep in flight — the reported failure window.
        let pending = sandboxed(Some(SandboxPrepState::Pending))
            .sandbox_spawn_block()
            .expect("a pending prep must block the spawn");
        assert!(
            pending.contains("full") && pending.contains("r1"),
            "the reason must name the profile and the run, got {pending:?}"
        );

        // No prep event yet: RunStarted and SandboxPrepStarted are ~100 ms apart, and
        // a read of the fresh run dir inside that window wakes the watcher. Blocking
        // is the fail-safe direction — a wrong block costs one replayed spawn, a wrong
        // pass costs a dead node.
        assert!(
            sandboxed(None).sandbox_spawn_block().is_some(),
            "a sandboxed Run whose prep has not even started must block"
        );
    }

    #[test]
    fn sandbox_spawn_block_never_gates_the_host_path() {
        // An `off` Run has no prep events at all, so `sandbox_prep` is permanently
        // `None`. Reading that as "not ready" would deadlock every non-sandboxed Run
        // in the instance — the `off` parcours must stay byte-identical.
        let mut s = RunState::new("r1".into(), "p".into());
        assert!(s.sandbox_spawn_block().is_none());
        // …and stays unblocked whatever the prep field says (defensive: `off` wins).
        s.sandbox_prep = Some(SandboxPrepState::Pending);
        assert!(s.sandbox_spawn_block().is_none());
    }

    #[test]
    fn projects_empty_events_to_none() {
        assert!(project(&[]).is_none());
    }

    #[test]
    fn lone_command_issued_projects_none() {
        // #328: a log fragment with no RunStarted (e.g. a late event that
        // slipped in around a forget) must never project as a phantom run.
        let events = vec![make_event_with_payload(
            EventKind::CommandIssued,
            None,
            serde_json::json!({ "kind": "extend_cycle" }),
        )];
        assert!(project(&events).is_none());
    }

    #[test]
    fn lone_node_event_projects_none() {
        // #328: same for node events — the exact ghost shape from the issue.
        let events = vec![make_event(EventKind::NodeStopped, Some("n1"), Some(1))];
        assert!(project(&events).is_none());
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

    /// #490 — the twin of the test above, for the `script` fail-fast shape, which
    /// nests everything under `detail`.
    #[test]
    fn projects_nested_detail_violations_of_a_script_fail_fast() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("silent"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("silent"),
                serde_json::json!({
                    "reason": "script output validation failed",
                    "detail": {
                        "kind": "frontmatter_mismatch",
                        "violations": [
                            { "port": "out", "field": "verdict", "reason": "not in allowed" },
                        ],
                    },
                }),
            ),
            make_event(EventKind::RunFailed, None, None),
        ];

        let node = &project(&events).unwrap().nodes["silent"];
        assert_eq!(node.frontmatter_violations.len(), 1);
        assert_eq!(node.frontmatter_violations[0]["field"], "verdict");
        assert!(
            node.missing_outputs.is_empty(),
            "exclusive-or by construction"
        );
    }

    /// The other half of the same nesting: a `script` node that never wrote its
    /// declared output.
    #[test]
    fn projects_nested_detail_missing_outputs_of_a_script_fail_fast() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("silent"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("silent"),
                serde_json::json!({
                    "reason": "script output validation failed",
                    "detail": { "kind": "missing_outputs", "missing": ["out", "log"] },
                }),
            ),
        ];

        let node = &project(&events).unwrap().nodes["silent"];
        assert_eq!(node.missing_outputs, vec!["out", "log"]);
        assert!(node.frontmatter_violations.is_empty());
    }

    /// A `MergeConflictDetected` also carries a `detail`, but it routes to
    /// `apply_merge_event` and never reaches the `NodeFailed` arm. Pinned so the
    /// `detail` lookup added by #490 cannot be read as a collision waiting to happen.
    #[test]
    fn a_merge_conflict_detail_never_lands_in_the_node_evidence() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("impl"), Some(1)),
            make_event_with_payload(
                EventKind::MergeConflictDetected,
                Some("impl"),
                serde_json::json!({
                    "reason": "conflict merging impl into pipeline branch",
                    "detail": "CONFLICT (content): Merge conflict in shared.txt",
                }),
            ),
        ];

        let node = &project(&events).unwrap().nodes["impl"];
        assert!(node.frontmatter_violations.is_empty());
        assert!(node.missing_outputs.is_empty());
    }

    /// A *successful* retry must leave no stale violations on the now-green node:
    /// `NodeStarted` resets the evidence vectors, not just `completed_at` /
    /// `failure_reason` (#490).
    #[test]
    fn a_new_attempt_purges_the_evidence_of_the_previous_one() {
        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(1)),
            make_event_with_payload(
                EventKind::NodeFailed,
                Some("reviewer"),
                serde_json::json!({
                    "reason": "output validation failed",
                    "violations": [{ "port": "review", "field": "verdict", "reason": "nope" }],
                    "missing": ["review"],
                }),
            ),
            make_event(EventKind::NodeStarted, Some("reviewer"), Some(2)),
            make_event(EventKind::NodeCompleted, Some("reviewer"), Some(2)),
        ];

        let node = &project(&events).unwrap().nodes["reviewer"];
        assert_eq!(node.status, NodeStatus::Completed);
        assert!(
            node.frontmatter_violations.is_empty(),
            "a green node must not carry the violations of the attempt it recovered from"
        );
        assert!(node.missing_outputs.is_empty());
    }

    fn interrupt_event(node_id: &str, iter: i64, reason: &str) -> Event {
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: EventKind::NodeInterrupted,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "reason": reason })),
        }
    }

    #[test]
    fn node_interrupt_parks_the_run_awaiting_user_with_the_reason() {
        // ADR-0049: session death → NodeInterrupted → finalize lifts a Running run
        // to AwaitingUser, carrying the incident reason distinctly.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            interrupt_event("worker", 1, "session_died: tmux gone"),
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert_eq!(state.nodes["worker"].status, NodeStatus::Interrupted);
        assert_eq!(
            state.awaiting_reason.as_deref(),
            Some("session_died: tmux gone")
        );
        // Distinct from an interactive wait: that carries no awaiting_reason.
        assert!(state.failure_reason.is_none());
    }

    #[test]
    fn node_interrupt_before_start_materialises_the_node() {
        // ADR-0050 §1: a spawn abort BEFORE NodeStarted still names its node; the
        // projection materialises it Interrupted (no iteration row) so the run
        // parks visibly instead of freezing `running`.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            interrupt_event("worker", 1, "failed to ensure sub-worktree"),
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert_eq!(state.nodes["worker"].status, NodeStatus::Interrupted);
        assert!(state.nodes["worker"].iterations.is_empty());
    }

    #[test]
    fn run_interrupted_parks_a_live_run_and_is_inert_on_a_terminal_one() {
        // A run-level give-up (stall / unrouted / merge) parks AwaitingUser…
        let parked = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event_with_payload(
                EventKind::RunInterrupted,
                None,
                serde_json::json!({ "reason": "run_stalled: nothing schedulable" }),
            ),
        ])
        .unwrap();
        assert_eq!(parked.status, RunStatus::AwaitingUser);
        assert_eq!(
            parked.awaiting_reason.as_deref(),
            Some("run_stalled: nothing schedulable")
        );

        // …but a RunInterrupted racing a genuine completion must NOT un-terminalize
        // it (#221): inert on an already-terminal run.
        let completed = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::RunCompleted, None, None),
            make_event_with_payload(
                EventKind::RunInterrupted,
                None,
                serde_json::json!({ "reason": "late" }),
            ),
        ])
        .unwrap();
        assert_eq!(completed.status, RunStatus::Completed);
        assert!(completed.awaiting_reason.is_none());
    }

    #[test]
    fn run_interrupted_carries_the_explicit_reason_code() {
        // A run-level give-up now writes reason_code + reason; both project.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event_with_payload(
                EventKind::RunInterrupted,
                None,
                interrupt_payload("unrouted", "unrouted: no live branch reaches End"),
            ),
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert_eq!(state.awaiting_reason_code.as_deref(), Some("unrouted"));
        assert_eq!(
            state.awaiting_reason.as_deref(),
            Some("unrouted: no live branch reaches End")
        );
    }

    #[test]
    fn run_interrupted_derives_the_code_from_a_legacy_prose_prefix() {
        // A historical log without the explicit reason_code key still surfaces a
        // machine code, parsed from the `<slug>: <prose>` prefix.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event_with_payload(
                EventKind::RunInterrupted,
                None,
                serde_json::json!({ "reason": "run_stalled: nothing schedulable" }),
            ),
        ])
        .unwrap();
        assert_eq!(state.awaiting_reason_code.as_deref(), Some("run_stalled"));
    }

    #[test]
    fn node_interrupt_lifts_a_machine_code_from_its_reason_prefix() {
        // finalize derives awaiting_reason_code from an interrupted node's
        // `<slug>: <prose>` reason — a node-level interrupt has no run-level
        // reason_code payload to read.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            interrupt_event("worker", 1, "session_died: tmux gone"),
        ])
        .unwrap();
        assert_eq!(state.awaiting_reason_code.as_deref(), Some("session_died"));
    }

    #[test]
    fn interactive_wait_has_no_reason_code() {
        // An interactive AwaitingUser (a node asking its user) carries no incident
        // reason and thus no code — the two awaiting causes stay distinguishable.
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("ask"), Some(1)),
            make_event(EventKind::NodeAwaitingUser, Some("ask"), Some(1)),
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::AwaitingUser);
        assert!(state.awaiting_reason.is_none());
        assert!(state.awaiting_reason_code.is_none());
    }

    #[test]
    fn reopen_clears_the_reason_code() {
        let reopen = Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: EventKind::CommandIssued,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "command": "reopen_run" })),
        };
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event_with_payload(
                EventKind::RunInterrupted,
                None,
                interrupt_payload("run_stalled", "run_stalled: nothing schedulable"),
            ),
            reopen,
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert!(state.awaiting_reason.is_none());
        assert!(state.awaiting_reason_code.is_none());
    }

    #[test]
    fn parse_reason_code_only_accepts_a_snake_case_prefix() {
        assert_eq!(
            parse_reason_code("session_died: gone").as_deref(),
            Some("session_died")
        );
        assert_eq!(
            parse_reason_code("region_exhausted: x").as_deref(),
            Some("region_exhausted")
        );
        // A prose sentence whose head is not a slug carries no code.
        assert!(parse_reason_code("exhausted — unrouted: region r").is_none());
        assert!(parse_reason_code("no colon here").is_none());
        assert!(parse_reason_code("Capitalized: x").is_none());
        assert!(parse_reason_code("slug: ").is_none());
    }

    #[test]
    fn region_state_kind_all_is_total() {
        // ALL must list every variant. A new region kind added to the enum makes
        // this match fail to compile until it is added here (and thus to the
        // open-region defer of run_stall_reason).
        for kind in RegionStateKind::ALL {
            match kind {
                RegionStateKind::Loop | RegionStateKind::ForEach | RegionStateKind::Collection => {}
            }
        }
        assert_eq!(RegionStateKind::ALL.len(), 3);
    }

    #[test]
    fn has_open_region_counts_every_kind() {
        // Each region-state map, in isolation, is seen by has_open_region /
        // open_region_ids — the #453 exhaustiveness guarantee.
        let mut loops = RunState::new("r".into(), "p".into());
        loops.loop_states.insert(
            "L".into(),
            LoopState {
                loop_node_id: "L".into(),
                current_iter: 1,
                max_iter: 3,
                break_received: false,
                done: false,
            },
        );
        assert!(loops.has_open_region());
        assert_eq!(loops.open_region_ids(), vec!["L".to_string()]);

        let mut fe = RunState::new("r".into(), "p".into());
        fe.foreach_states.insert(
            "F".into(),
            ForEachState {
                foreach_node_id: "F".into(),
                total_items: 2,
                break_received: false,
                done: false,
            },
        );
        assert!(fe.has_open_region());

        let mut coll = RunState::new("r".into(), "p".into());
        coll.collection_states.insert(
            "C".into(),
            CollectionState {
                region_id: "C".into(),
                total_items: 2,
                done: false,
                entry: "m".into(),
                members: vec!["m".into()],
            },
        );
        assert!(coll.has_open_region());

        // A done region does not hold the run open.
        coll.collection_states.get_mut("C").unwrap().done = true;
        assert!(!coll.has_open_region());
        assert!(coll.open_region_ids().is_empty());
    }

    #[test]
    fn reopen_lifts_a_terminal_run_and_re_drives_interrupted_nodes() {
        // AC6/AC8/FP#8: reopen a terminal run → Running, satisfied nodes stay
        // Completed (never re-spawned, anti-#221), interrupted nodes are dropped so
        // they re-drive; the terminal label stays in the log.
        let reopen = Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: EventKind::CommandIssued,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "command": "reopen_run" })),
        };
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("done"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("done"), Some(1)),
            make_event(EventKind::NodeStarted, Some("hurt"), Some(1)),
            interrupt_event("hurt", 1, "session_died"),
            make_event_with_payload(
                EventKind::RunFailed,
                None,
                serde_json::json!({ "reason": "human abandon" }),
            ),
            reopen,
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.nodes["done"].status, NodeStatus::Completed);
        assert!(
            !state.nodes.contains_key("hurt"),
            "the interrupted node is dropped so the scheduler re-drives it fresh"
        );
        assert!(state.failure_reason.is_none() && state.awaiting_reason.is_none());
    }

    #[test]
    fn legacy_resume_run_command_still_reopens_a_completed_run() {
        // Back-compat: the historical `resume_run` command string re-opens exactly
        // like `reopen_run` (append-only log: every replayed run must still lift).
        let resume = Event {
            id: None,
            run_id: "run-1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: EventKind::CommandIssued,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "command": "resume_run" })),
        };
        let state = project(&[
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("a"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("a"), Some(1)),
            make_event(EventKind::RunCompleted, None, None),
            resume,
        ])
        .unwrap();
        assert_eq!(state.status, RunStatus::Running);
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
        // foreign file write) and must NEVER un-terminalize a completed run.
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

    fn start_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "start", "node_type": "start", "inputs": [], "outputs": [{"name": "user_prompt", "side": "right"}] })
    }

    fn end_node_def() -> serde_json::Value {
        serde_json::json!({ "id": "end", "node_type": "end", "inputs": [{"name": "result", "side": "left"}], "outputs": [] })
    }

    fn node_def(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "node_type": "agent", "isolated_worktree": false,
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
                "pipeline_name": "fan-out",
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
    fn replayed_junk_extend_cycle_leaves_projection_identical() {
        // ADR-0025 / #327 replay safety: validate-before-append means NEW logs
        // never gain an extend_cycle for an unknown node, but OLD logs may
        // already contain one. Its consumers are inert for unknown keys, so the
        // projection must be identical with or without the junk command.
        let base = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
        ];
        let mut with_junk = base.clone();
        with_junk.insert(
            2,
            Event {
                kind: EventKind::CommandIssued,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": "no-such-node",
                    "additional_iter": 3,
                })),
                ..make_event(EventKind::CommandIssued, None, None)
            },
        );
        let clean = project(&base).unwrap();
        let junked = project(&with_junk).unwrap();
        assert_eq!(clean.status, junked.status);
        assert_eq!(clean.nodes.len(), junked.nodes.len());
        assert_eq!(clean.nodes["worker"].status, junked.nodes["worker"].status);
        assert_eq!(clean.loop_states.len(), junked.loop_states.len());
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

    #[test]
    fn collection_full_lifecycle_3_items() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "collection" }),
            ),
            make_event_with_payload(
                EventKind::CollectionStarted,
                None,
                serde_json::json!({ "region_id": "fan", "entry": "worker", "total_items": 3 }),
            ),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(2)),
            make_event(EventKind::NodeStarted, Some("worker"), Some(3)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(2)),
            make_event(EventKind::NodeCompleted, Some("worker"), Some(3)),
            make_event_with_payload(
                EventKind::CollectionDone,
                None,
                serde_json::json!({ "region_id": "fan" }),
            ),
        ];

        let state = project(&events).unwrap();
        let cs = &state.collection_states["fan"];
        assert_eq!(cs.total_items, 3);
        assert!(cs.done);
        assert_eq!(state.nodes["worker"].iterations.len(), 3);
    }

    #[test]
    fn collection_empty_projects_done_with_zero_items() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "collection-empty" }),
            ),
            make_event_with_payload(
                EventKind::CollectionEmpty,
                None,
                serde_json::json!({ "region_id": "fan" }),
            ),
            make_event_with_payload(
                EventKind::CollectionDone,
                None,
                serde_json::json!({ "region_id": "fan" }),
            ),
        ];

        let state = project(&events).unwrap();
        let cs = &state.collection_states["fan"];
        assert_eq!(cs.total_items, 0);
        assert!(cs.done);
    }

    #[test]
    fn collection_events_with_malformed_payload_are_ignored() {
        // Panic-free projection: missing region_id / no payload are no-ops.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "collection-bad" }),
            ),
            make_event(EventKind::CollectionStarted, None, None),
            make_event_with_payload(
                EventKind::CollectionDone,
                None,
                serde_json::json!({ "totally": "unrelated" }),
            ),
        ];
        let state = project(&events).unwrap();
        assert!(state.collection_states.is_empty());
    }

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

    /// #465 (ADR-0042): `RunStarted.target_repos` projects into
    /// `RunState.target_repos`; a legacy payload without the key stays empty
    /// (mono-repo); a malformed value degrades to empty with a `warn!` and NEVER
    /// panics — `project()` runs before the transition guard.
    #[test]
    fn target_repos_projects_and_degrades_gracefully() {
        // Present + well-formed → projected verbatim.
        let with = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "target_repo": "/repos/primary",
                "target_repos": [
                    { "repo": "/repos/secondary", "alias": "secondary",
                      "sha": "deadbeef", "base_branch": "main" }
                ],
            }),
        )];
        let state = project(&with).unwrap();
        assert_eq!(state.target_repos.len(), 1);
        assert_eq!(state.target_repos[0].repo, "/repos/secondary");
        assert_eq!(state.target_repos[0].alias, "secondary");
        assert_eq!(state.target_repos[0].sha, "deadbeef");
        assert_eq!(state.target_repos[0].base_branch.as_deref(), Some("main"));
        // ADR-0047: a payload without `read_only` projects `false` (writable).
        assert!(
            !state.target_repos[0].read_only,
            "an absent read_only key must read as writable (ADR-0047 decision 2)"
        );

        // Legacy payload (no key) → empty, mono-repo.
        let legacy = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p", "target_repo": "/repos/primary" }),
        )];
        assert!(project(&legacy).unwrap().target_repos.is_empty());

        // Malformed value → empty, NO panic (degrade with warn).
        let malformed = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p", "target_repos": "not-an-array" }),
        )];
        assert!(project(&malformed).unwrap().target_repos.is_empty());
    }

    /// ADR-0047: the `read_only` opt-in projects through both `RunStarted` and
    /// `RunReposEdited`, defaults to `false` when the key is absent, and a writable
    /// pin serialises byte-identically to a pre-flag pin (no `read_only` key).
    #[test]
    fn read_only_flag_projects_defaults_false_and_skips_when_writable() {
        // Explicit `read_only: true` via RunStarted → projected true.
        let started = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({
                "pipeline_name": "p",
                "target_repo": "/repos/primary",
                "target_repos": [
                    { "repo": "/repos/ro", "alias": "ro", "sha": "aaaa", "read_only": true },
                    { "repo": "/repos/rw", "alias": "rw", "sha": "bbbb" }
                ],
            }),
        )];
        let state = project(&started).unwrap();
        assert_eq!(state.target_repos.len(), 2);
        assert!(
            state.target_repos[0].read_only,
            "explicit true projects true"
        );
        assert!(
            !state.target_repos[1].read_only,
            "absent key projects writable"
        );

        // `RunReposEdited` carries the flag too.
        let mut edited = started.clone();
        edited.push(make_event_with_payload(
            EventKind::RunReposEdited,
            None,
            serde_json::json!({
                "target_repos": [
                    { "repo": "/repos/rw", "alias": "rw", "sha": "bbbb", "read_only": true }
                ]
            }),
        ));
        let state = project(&edited).unwrap();
        assert_eq!(state.target_repos.len(), 1);
        assert!(
            state.target_repos[0].read_only,
            "RunReposEdited must project read_only"
        );

        // serde skip: a writable pin round-trips without a `read_only` key, byte-
        // identical to a pre-ADR-0047 pin; a read-only pin carries the key.
        let writable = RepoPin {
            repo: "/r".into(),
            alias: "r".into(),
            sha: "c".into(),
            base_branch: None,
            read_only: false,
        };
        let json = serde_json::to_value(&writable).unwrap();
        assert!(
            json.get("read_only").is_none(),
            "a writable pin must not serialise a read_only key"
        );
        let read_only = RepoPin {
            read_only: true,
            ..writable.clone()
        };
        assert_eq!(
            serde_json::to_value(&read_only).unwrap().get("read_only"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    /// #551 (ADR-0046): the `RunStarted.harness` freeze projects into
    /// `RunState.harness`; an absent key (every historical Run, every Run with no
    /// explicit choice) stays `None`; an empty string never wins a tier (#347).
    #[test]
    fn harness_projects_and_defaults_to_none() {
        // Present + non-empty → frozen verbatim.
        let with = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p", "harness": "opencode" }),
        )];
        assert_eq!(project(&with).unwrap().harness.as_deref(), Some("opencode"));

        // Absent key → None (resolve through the instance default and the floor).
        let legacy = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p" }),
        )];
        assert_eq!(project(&legacy).unwrap().harness, None);

        // Empty string → None: a blank freeze can never win a precedence tier (#347).
        let blank = vec![make_event_with_payload(
            EventKind::RunStarted,
            None,
            serde_json::json!({ "pipeline_name": "p", "harness": "" }),
        )];
        assert_eq!(project(&blank).unwrap().harness, None);
    }

    /// #465 slice 2 (ADR-0042): `RunReposEdited` overwrites `target_repos`
    /// wholesale with the re-frozen active list — the reducer mirrors the
    /// `RunStarted` arm, so a mono-repo Run can grow secondaries mid-run and a
    /// multi-repo Run can shed them, and the last edit wins.
    #[test]
    fn run_repos_edited_overwrites_the_active_list() {
        // A Run that started mono-repo gains a secondary.
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p", "target_repo": "/repos/primary" }),
            ),
            make_event_with_payload(
                EventKind::RunReposEdited,
                None,
                serde_json::json!({
                    "target_repos": [
                        { "repo": "/repos/lib", "alias": "lib", "sha": "cafe1234",
                          "base_branch": "main" }
                    ]
                }),
            ),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.target_repos.len(), 1);
        assert_eq!(state.target_repos[0].alias, "lib");
        assert_eq!(state.target_repos[0].sha, "cafe1234");

        // A second edit REPLACES the list (not appends) — removal is the empty list.
        let mut removed = events.clone();
        removed.push(make_event_with_payload(
            EventKind::RunReposEdited,
            None,
            serde_json::json!({ "target_repos": [] }),
        ));
        assert!(project(&removed).unwrap().target_repos.is_empty());
    }

    /// #465 slice 2 / #221 (double guard, reducer half): a `RunReposEdited`
    /// appended AFTER a terminal event must not touch the frozen list and, above
    /// all, must not un-terminalize the Run — the same invariant
    /// `apply_pipeline_event` holds against a stray `PipelineModified`.
    #[test]
    fn run_repos_edited_after_terminal_is_inert() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "p",
                    "target_repo": "/repos/primary",
                    "target_repos": [
                        { "repo": "/repos/lib", "alias": "lib", "sha": "aaaa1111" }
                    ],
                }),
            ),
            make_event(EventKind::RunCompleted, None, None),
            // The passive edit races in after completion.
            make_event_with_payload(
                EventKind::RunReposEdited,
                None,
                serde_json::json!({
                    "target_repos": [
                        { "repo": "/repos/other", "alias": "other", "sha": "bbbb2222" }
                    ]
                }),
            ),
        ];
        let state = project(&events).unwrap();
        assert_eq!(
            state.status,
            RunStatus::Completed,
            "RunReposEdited after RunCompleted must NOT reopen the run (#221)"
        );
        assert!(state.completed_at.is_some());
        assert_eq!(
            state.target_repos.len(),
            1,
            "the frozen list must survive a post-terminal edit"
        );
        assert_eq!(
            state.target_repos[0].alias, "lib",
            "the terminal Run keeps its original secondary, not the racing edit"
        );
    }

    /// A malformed `RunReposEdited` payload keeps the PREVIOUS list (never resets
    /// it to empty) and never panics — a soft failure must not silently strand a
    /// live node reading a snapshot the projection would otherwise forget.
    #[test]
    fn run_repos_edited_malformed_keeps_previous_list() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({
                    "pipeline_name": "p",
                    "target_repo": "/repos/primary",
                    "target_repos": [
                        { "repo": "/repos/lib", "alias": "lib", "sha": "aaaa1111" }
                    ],
                }),
            ),
            make_event_with_payload(
                EventKind::RunReposEdited,
                None,
                serde_json::json!({ "target_repos": "not-an-array" }),
            ),
        ];
        let state = project(&events).unwrap();
        assert_eq!(state.target_repos.len(), 1);
        assert_eq!(state.target_repos[0].alias, "lib");
    }

    /// #503: a Run's whole failure signal used to be a red dot — every
    /// `RunFailed` carried a `reason` and nothing read it.
    #[test]
    fn a_failed_run_carries_its_reason() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event_with_payload(
                EventKind::RunFailed,
                None,
                serde_json::json!({ "reason": "merge conflict on ship: 20 conflicting file(s)" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        assert_eq!(
            state.failure_reason.as_deref(),
            Some("merge conflict on ship: 20 conflicting file(s)")
        );
    }

    /// A blank reason is *absence*: an explanation box with nothing in it reads
    /// worse than the red dot it replaces.
    #[test]
    fn a_blank_run_reason_is_no_reason() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event_with_payload(
                EventKind::RunFailed,
                None,
                serde_json::json!({ "reason": "   " }),
            ),
        ];
        assert!(project(&events).unwrap().failure_reason.is_none());
    }

    /// A halt says `message`, a skip says `reason` — all three non-green terminals
    /// answer "why?" through the one field a UI can render.
    #[test]
    fn a_halt_and_a_skip_also_carry_their_reason() {
        for (kind, payload, expected) in [
            (
                EventKind::RunHalted,
                serde_json::json!({ "message": "stop condition met" }),
                "stop condition met",
            ),
            (
                EventKind::RunSkipped,
                serde_json::json!({ "reason": "eligible pool was empty" }),
                "eligible pool was empty",
            ),
        ] {
            let events = vec![
                make_event_with_payload(
                    EventKind::RunStarted,
                    None,
                    serde_json::json!({ "pipeline_name": "p" }),
                ),
                make_event_with_payload(kind, None, payload),
            ];
            assert_eq!(
                project(&events).unwrap().failure_reason.as_deref(),
                Some(expected)
            );
        }
    }

    /// A Run being driven again must not still display last time's cause — same
    /// rule `NodeStarted` applies to a node's `failure_reason`.
    #[test]
    fn resuming_clears_the_previous_run_failure_reason() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event_with_payload(
                EventKind::RunFailed,
                None,
                serde_json::json!({ "reason": "merge conflict on ship" }),
            ),
            make_event(EventKind::RunResumed, None, None),
        ];
        assert!(project(&events).unwrap().failure_reason.is_none());
    }

    /// #503: informational, exactly like `MergeConflictDetected` — the completion
    /// carries on and moves the node itself.
    #[test]
    fn resolving_a_merge_back_in_the_node_favour_touches_no_state() {
        let events = vec![
            make_event_with_payload(
                EventKind::RunStarted,
                None,
                serde_json::json!({ "pipeline_name": "p" }),
            ),
            make_event(EventKind::NodeStarted, Some("ship"), Some(1)),
            make_event_with_payload(
                EventKind::MergeResolvedInNodeFavour,
                Some("ship"),
                serde_json::json!({ "merge_commit": "deadbeef" }),
            ),
        ];

        let state = project(&events).unwrap();
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.nodes["ship"].status, NodeStatus::Running);
        assert!(state.failure_reason.is_none());
    }

    #[test]
    fn event_kind_serialization_roundtrip() {
        let kinds = vec![
            EventKind::NodeStopped,
            EventKind::NodeAutoCompleted,
            EventKind::NodeStale,
            EventKind::NodeBlockedOnLimit,
            EventKind::NodeAutoCompleteObserved,
            EventKind::MergeResolvedInNodeFavour,
            EventKind::RunPaused,
            EventKind::RunResumed,
        ];
        let expected_strings = vec![
            "\"node_stopped\"",
            "\"node_auto_completed\"",
            "\"node_stale\"",
            "\"node_blocked_on_limit\"",
            "\"node_auto_complete_observed\"",
            "\"merge_resolved_in_node_favour\"",
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

    fn node_state(
        id: &str,
        status: NodeStatus,
        iter: i64,
        iters: &[(i64, NodeStatus)],
    ) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            harness: None,
            isolated_worktree: None,
            cost: None,
            node_id: id.to_string(),
            status,
            iter,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
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
            missing_outputs: Vec::new(),
            delivery: None,
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
            Skipped,
            Failed,
            Stopped,
            Stale,
            Interrupted,
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
                // `Interrupted` (ADR-0049): holds no session and cannot progress
                // on its own — a human resumes/restarts it, so it parks the run
                // `AwaitingUser` rather than keeping it schedulable.
                Pending | Completed | Skipped | Failed | Stopped | Stale | Interrupted => {
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
    fn completed_iters_quarantines_a_failed_iter_in_the_middle_of_the_set() {
        // #353: a repeated/pooled source that failed iter 2 must pool iters 1
        // and 3 only — the failed iter's artifact stays on disk but is never
        // resolvable.
        let s = run_with(vec![node_state(
            "reviewer",
            NodeStatus::Completed,
            3,
            &[
                (1, NodeStatus::Completed),
                (2, NodeStatus::Failed),
                (3, NodeStatus::Completed),
            ],
        )]);
        assert_eq!(s.completed_iters("reviewer"), vec![1, 3]);
    }

    #[test]
    fn completed_iters_falls_back_to_head_when_history_is_empty() {
        // Legacy state: head status Completed, no per-iteration history.
        let s = run_with(vec![node_state("a", NodeStatus::Completed, 4, &[])]);
        assert_eq!(s.completed_iters("a"), vec![4]);
    }

    #[test]
    fn completed_iters_is_empty_when_nothing_completed_or_node_absent() {
        let running = run_with(vec![node_state(
            "a",
            NodeStatus::Running,
            1,
            &[(1, NodeStatus::Running)],
        )]);
        assert_eq!(running.completed_iters("a"), Vec::<i64>::new());
        assert_eq!(running.completed_iters("ghost"), Vec::<i64>::new());
    }

    #[test]
    fn completed_iters_last_equals_latest_completed_iter() {
        // Documented invariant: the set's max is the single-input resolution.
        let s = run_with(vec![node_state(
            "a",
            NodeStatus::Completed,
            3,
            &[
                (1, NodeStatus::Completed),
                (2, NodeStatus::Failed),
                (3, NodeStatus::Completed),
            ],
        )]);
        assert_eq!(
            s.completed_iters("a").last().copied(),
            s.latest_completed_iter("a")
        );
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
                    "pipeline_id": "lib-golden-pipe",
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
            // Collection region fan1: started -> done (ADR-0011 / #269), and
            // fan2: empty list short-circuits to done.
            ev(
                EventKind::CollectionStarted,
                None,
                None,
                "2026-02-01T00:00:09.000Z",
                Some(
                    serde_json::json!({ "region_id": "fan1", "entry": "worker", "total_items": 2 }),
                ),
            ),
            ev(
                EventKind::CollectionDone,
                None,
                None,
                "2026-02-01T00:00:10.000Z",
                Some(serde_json::json!({ "region_id": "fan1" })),
            ),
            ev(
                EventKind::CollectionEmpty,
                None,
                None,
                "2026-02-01T00:00:11.000Z",
                Some(serde_json::json!({ "region_id": "fan2" })),
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
            "collection_states": {
                // #453: `fan1`'s CollectionStarted payload predates `members`
                // (it carries `entry` only) — the projection falls back to the
                // entry alone, which is the whole region for a single-member
                // collection. `fan2` never fanned out (CollectionEmpty), so its
                // shape is unknown and both fields stay empty.
                "fan1": { "done": true, "region_id": "fan1", "total_items": 2, "entry": "worker", "members": ["worker"] },
                "fan2": { "done": true, "region_id": "fan2", "total_items": 0, "entry": "", "members": [] }
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
                { "id": "planner", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "worker", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "auto", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "stopped", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "stale", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "temp", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "interactive", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null },
                { "id": "sw", "inputs": [ { "name": "task", "side": "left" } ], "node_type": "agent", "isolated_worktree": false, "outputs": [ { "name": "out", "side": "right" } ], "view_x": null, "view_y": null }
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
                // #490: `worker` failed iter 1 on a frontmatter mismatch, then
                // succeeded on iter 2. It carries NO `frontmatter_violations` any
                // more — `NodeStarted` purges the evidence vectors, so a green node
                // no longer shows the violations of the attempt it recovered from.
                // This is the ONE justified edit to this golden: `missing_outputs`
                // is `skip_serializing_if`-empty everywhere, so every other byte is
                // unchanged — which is the wire-compatibility proof.
                "worker": {
                    "completed_at": "2026-02-01T00:02:03.000Z", "failure_reason": null, "frontmatter_retries": 0,
                    "iter": 2,
                    "iterations": [
                        { "completed_at": "2026-02-01T00:02:01.000Z", "iter": 1, "started_at": "2026-02-01T00:02:00.000Z", "status": "failed" },
                        { "completed_at": "2026-02-01T00:02:03.000Z", "iter": 2, "started_at": "2026-02-01T00:02:02.000Z", "status": "completed" }
                    ],
                    "node_id": "worker", "started_at": "2026-02-01T00:02:02.000Z", "status": "completed"
                }
            },
            "pipeline_id": "lib-golden-pipe",
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
            "triggered_by": "trigger-7",
            "sandbox": "off"
        });
        assert_eq!(actual, expected);
    }

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
    fn set_region_max_iter_folds_an_absolute_override_last_write_wins() {
        // #600: `set_region_max_iter` is absolute and last-write-wins (unlike the
        // additive `bump_region`), keyed by region id.
        let mut state = RunState::new("r".into(), String::new());
        apply_command_event(
            &mut state,
            &make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "set_region_max_iter", "region_id": "R", "max_iter": 8 }),
            ),
        );
        assert_eq!(state.region_max_iter_overrides.get("R"), Some(&8));
        apply_command_event(
            &mut state,
            &make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "set_region_max_iter", "region_id": "R", "max_iter": 3 }),
            ),
        );
        assert_eq!(
            state.region_max_iter_overrides.get("R"),
            Some(&3),
            "last write wins, not accumulated"
        );
    }

    #[test]
    fn set_region_max_iter_ignores_a_non_positive_cap() {
        let mut state = RunState::new("r".into(), String::new());
        apply_command_event(
            &mut state,
            &make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "set_region_max_iter", "region_id": "R", "max_iter": 0 }),
            ),
        );
        assert!(state.region_max_iter_overrides.is_empty());
    }

    #[test]
    fn force_route_folds_a_forced_target_keyed_by_source() {
        // #600: a `force_route` is keyed by its source (node OR region id) → target.
        let mut state = RunState::new("r".into(), String::new());
        apply_command_event(
            &mut state,
            &make_event_with_payload(
                EventKind::CommandIssued,
                None,
                serde_json::json!({ "command": "force_route", "from": "rev", "target": "end" }),
            ),
        );
        assert_eq!(state.forced_routes.get("rev"), Some(&"end".to_string()));
    }

    #[test]
    fn a_skip_completion_creates_a_never_started_node_as_skipped() {
        // #600/#620: skipping a node that never started (pruned as structurally
        // unreachable) marks it satisfied — the applier creates it directly as
        // terminal, never a session-less Running window. #620: the status is
        // `Skipped` (not `Completed`), so the canvas greys the pruned node, and the
        // prune `reason` is lifted onto `skip_reason` at node level.
        let mut state = RunState::new("r".into(), String::new());
        apply_node_event(
            &mut state,
            &make_event_with_payload(
                EventKind::NodeCompleted,
                Some("orphan"),
                serde_json::json!({ "skipped": true, "reason": "unreachable" }),
            ),
        );
        assert_eq!(state.nodes["orphan"].status, NodeStatus::Skipped);
        assert_eq!(
            state.nodes["orphan"].skip_reason.as_deref(),
            Some("unreachable"),
            "the prune reason reads at node level"
        );
        assert_eq!(state.nodes["orphan"].iterations.len(), 1);
        assert_eq!(
            state.nodes["orphan"].iterations[0].status,
            NodeStatus::Skipped
        );
        // A `Skipped` node is a settled completion — it satisfies the run-completion
        // gate exactly like `Completed`, so the run can still terminate.
        assert!(
            state.all_nodes_completed(&["orphan".into()]),
            "a pruned node counts as done for run completion"
        );
    }

    #[test]
    fn a_plain_completion_of_a_never_started_node_is_a_projection_noop() {
        // Without the skip marker, a stray NodeCompleted for an absent node stays a
        // no-op (the transition guard rejects it upstream; this proves the applier
        // does not silently materialise phantom Completed nodes).
        let mut state = RunState::new("r".into(), String::new());
        apply_node_event(
            &mut state,
            &make_event(EventKind::NodeCompleted, Some("ghost"), Some(1)),
        );
        assert!(!state.nodes.contains_key("ghost"));
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
