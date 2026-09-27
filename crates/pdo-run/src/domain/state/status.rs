//! The statuses a Run and its nodes move through.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
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

/// How a Run is isolated — owned by the sandbox context, carried on `RunStarted` and
/// projected into [`RunState::sandbox`].
pub use pdo_sandbox::domain::mode::SandboxMode;

/// Wire tokens of a dropped two-position switch. Don't remove this mapping: `parse`
/// accepts any non-blank token as a profile name, so a historical payload carrying
/// one would project to `Profile("copy")` — an unknown profile — and fail the Run
/// hard. NOT consulted anywhere the user can still type a value: a stored `copy`
/// fails loud at the create chokepoint (ADR-0031 §7).
pub(crate) const LEGACY_SANDBOX_TOKENS: &[&str] = &["copy", "pure"];

/// Visibility of a sandboxed Run's one-time image preparation (#410). Additive to
/// [`RunState`]: `status` is untouched (stays `Running`), so no status consumer is
/// affected. Absent for `off` Runs and for historical/host runs. Projected from the
/// additive [`EventKind::SandboxPrepStarted`]/[`EventKind::SandboxPrepReady`] pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPrepState {
    /// The image is being pulled/built and the container has not yet received a session.
    Pending,
    /// The image is ready; the first session is about to spawn (or already has).
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IterationInfo {
    pub iter: i64,
    pub status: NodeStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completion_released: bool,
}
