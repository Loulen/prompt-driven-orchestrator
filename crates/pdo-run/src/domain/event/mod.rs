//! A Run's events: what the append-only event log records, one [`Event`] per
//! fact, typed by its [`EventKind`]. The wire shape of an event is the one `/ws`
//! and `GET /runs/{id}/events` serve (frozen contract).

mod graph;

pub use graph::{
    edge_info_from_pipeline, node_def_from_pipeline, EdgeInfo, NodeDefInfo, PortBrief,
};

use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The key of a payload naming the Actor who made the event.
pub const ACTOR: &str = "actor";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStarted,
    NodeStarted,
    /// The node is ready to run but throttled by the global session cap: it
    /// holds no tmux session yet and waits for an admission slot (#159).
    NodeWaiting,
    /// The node's agent **declared** a wait on its user (`pdo wait-user`, #588 /
    /// ADR-0069), or the daemon declared it for the agent when it refused an
    /// unreleased `pdo complete`. Payload: `cause` (`declared` /
    /// `completion_not_released` / `children_pending`) and an optional prose
    /// `message` — the question, shown as the banner reason. Never emitted at
    /// spawn: a fresh interactive node has the colour of a working node.
    NodeAwaitingUser,
    /// The wait was **lifted** by a human keystroke: an Enter that crossed the
    /// UI's PTY bridge after at least one typed byte (ADR-0069 §1). Projects the
    /// node back to `Running` and clears its awaiting info. Wire form:
    /// `"node_resumed"`. The other lift — the completion release — has its own
    /// event (`NodeCompletionReleased`).
    NodeResumed,
    NodeCompletionReleased,
    /// An Actor took the hand on the terminal of the node's session: it
    /// attached first and drives it, or took control from the pilot. Written
    /// when the pilot changes, never when the same Actor attaches again.
    /// Payload: `actor`, `source` (`pty`). Informational: projects nothing.
    /// Wire form: `"node_taken_over"`.
    NodeTakenOver,
    /// An Actor typed into the terminal of the node's session: one event per
    /// turn of speech, its first input after a take-over or a wait, never one
    /// per keystroke. An answer to a wait is the `NodeResumed` its Enter
    /// writes, not this. Payload: `actor`, `source` (`pty`). Informational:
    /// projects nothing. Wire form: `"node_typed"`.
    NodeTyped,
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
    /// The retired merge resolver's lifecycle (ADR-0082). Never emitted any
    /// more; kept so archived Runs that recorded it still deserialise, and so
    /// their cost keeps its Infrastructure line.
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
    /// Informational (manager on demand): a Pipeline Manager session was
    /// **reserved** for this Run. Written by every spawn path (the automatic
    /// spawn at create when `manager_enabled` resolves on, and the manual start
    /// endpoint) — the durable event lands FIRST, the tmux spawn after, so the
    /// orphan sweep's reservation-before-spawn invariant (ADR-0038) holds for a
    /// mid-Run start too. Projects nothing: the wire's `has_manager` is an
    /// OBSERVED fact (the live tmux session), never derived from this log — a
    /// spawn that failed after the reservation reads `false`, not `true`.
    /// Wire form: `"manager_started"`.
    ManagerStarted,
    /// Informational (manager on demand): the Run's Pipeline Manager session
    /// was stopped by the user (the Manager tab's Stop control). A stop on a
    /// session that is already gone is a calm no-op that still writes the event.
    /// Projects nothing — same observed-fact discipline as
    /// [`EventKind::ManagerStarted`]. Wire form: `"manager_stopped"`.
    ManagerStopped,
    /// A review comment left `sent` (#750, ADR-0067 §2): the operator handed a
    /// remark anchored on one diff line to the Pipeline Manager. The payload is
    /// the whole [`crate::domain::review_comment::ReviewComment`] (id, anchor, ref pair
    /// with SHAs, text, hunk excerpt, author, batch). Folded into
    /// `RunState::review_comments`, **additively on a terminal Run too** — review
    /// happens post-mortem, so this never re-opens anything. A sent comment is
    /// immutable: no event edits or deletes it. Wire form: `"review_comment_sent"`.
    ReviewCommentSent,
    /// An agent's reply to a sent comment (#751 emits it via `pdo review reply`;
    /// the projection is ready here). Payload: `id`, `author`, `text`,
    /// `proposes_resolution`. Wire form: `"review_comment_replied"`.
    ReviewCommentReplied,
    /// A comment resolved (by the human, or by the agent under the
    /// `review_agent_can_resolve` setting — #751). Payload: `id`. Wire form:
    /// `"review_comment_resolved"`.
    ReviewCommentResolved,
    /// A resolved comment reopened by the human (#751). Payload: `id`. Wire form:
    /// `"review_comment_reopened"`.
    ReviewCommentReopened,
    CommandIssued,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "DaemonEvent")]
pub struct Event {
    pub id: Option<i64>,
    pub run_id: String,
    pub ts: String,
    pub kind: EventKind,
    pub node_id: Option<String>,
    pub iter: Option<i64>,
    #[ts(type = "Record<string, unknown> | null")]
    pub payload: Option<serde_json::Value>,
}

impl Event {
    /// This event with `actor` as who made it (`payload.actor`, the shape of
    /// every gesture's event), unless it already names one. A payload that
    /// is not an object is left as it is.
    pub fn with_actor(mut self, actor: &Actor) -> Self {
        let actor = serde_json::to_value(actor).unwrap_or(serde_json::Value::Null);
        match self.payload.as_mut() {
            None => self.payload = Some(serde_json::json!({ ACTOR: actor })),
            Some(serde_json::Value::Object(payload)) => {
                payload.entry(ACTOR).or_insert(actor);
            }
            Some(_) => {}
        }
        self
    }

    /// The Actor who made this event (`payload.actor`), `None` for an event
    /// no gesture made, or one written before gestures named their Actor.
    pub fn actor(&self) -> Option<Actor> {
        let actor = self.payload.as_ref()?.get(ACTOR)?;
        serde_json::from_value(actor.clone()).ok()
    }

    /// The message that announces this event on the bus, exactly as `/ws`
    /// relays it to the front (D20): `{"type": "event", "event": …}`.
    pub fn bus_message(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "event",
            "event": self,
        })
    }

    /// Whether this event moves a node through its lifecycle, the events the
    /// transition guard validates before they are appended.
    pub fn is_node_transition(&self) -> bool {
        matches!(
            self.kind,
            EventKind::NodeStarted
                | EventKind::NodeWaiting
                | EventKind::NodeCompleted
                | EventKind::NodeAutoCompleted
                | EventKind::NodeStale
                | EventKind::NodeFailed
                | EventKind::NodeInterrupted
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(payload: Option<serde_json::Value>) -> Event {
        Event {
            id: None,
            run_id: "r1".into(),
            ts: "2026-11-07T10:00:00Z".into(),
            kind: EventKind::NodeStopped,
            node_id: Some("n1".into()),
            iter: Some(1),
            payload,
        }
    }

    fn alice() -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        }
    }

    #[test]
    fn an_event_takes_its_actor_beside_its_payload() {
        let stamped = event(Some(serde_json::json!({ "reason": "x" }))).with_actor(&alice());
        assert_eq!(
            stamped.payload,
            Some(serde_json::json!({
                "reason": "x",
                "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
            }))
        );
        assert_eq!(stamped.actor(), Some(alice()));
    }

    #[test]
    fn an_event_without_payload_gets_one_with_its_actor() {
        let stamped = event(None).with_actor(&Actor::LocalOperator);
        assert_eq!(
            stamped.payload,
            Some(serde_json::json!({ "actor": { "kind": "local_operator" } }))
        );
    }

    #[test]
    fn an_actor_already_named_is_kept() {
        let stamped = event(Some(
            serde_json::json!({ "actor": { "kind": "local_operator" } }),
        ))
        .with_actor(&alice());
        assert_eq!(stamped.actor(), Some(Actor::LocalOperator));
    }

    #[test]
    fn an_event_written_before_actors_reads_without_one() {
        assert_eq!(event(None).actor(), None);
        assert_eq!(
            event(Some(serde_json::json!({ "source": "pty_enter" }))).actor(),
            None
        );
        let old: Event = serde_json::from_value(serde_json::json!({
            "id": 7, "run_id": "r1", "ts": "2026-01-01T00:00:00Z",
            "kind": "node_resumed", "node_id": "n1", "iter": 1,
            "payload": { "source": "pty_enter" },
        }))
        .unwrap();
        assert_eq!(old.actor(), None);
    }
}
