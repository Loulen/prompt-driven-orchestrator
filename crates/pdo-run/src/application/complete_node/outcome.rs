//! What an attempt to complete a node answers, and where it came from.

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{info, warn};

use crate::application::scheduler::advance as run_advance;
use crate::domain::{completion_refusal, event};

/// What the shared completion head decided, in the two shapes an HTTP caller must
/// answer differently (ADR-0035).
///
/// Don't collapse the two: a **no-op is a legal duplicate and stays a `2xx`**, a
/// **reject is a refusal and is never a `2xx`**. Handing back a pre-projected
/// response the caller cannot tell apart is what made the invariant inexpressible.
pub enum CompletionHeadStop {
    /// Valid but nothing to do. `200 {"ok":true,"noop":true,"reason":…}`, exit `0`.
    NoOp { reason: String },
    /// The completion is not granted. Projected by `refusal_response`, so it
    /// cannot be a `2xx` by construction.
    Refused {
        refusal: completion_refusal::CompletionRefusal,
    },
}

impl CompletionHeadStop {
    pub fn into_response(self) -> Response {
        match self {
            CompletionHeadStop::NoOp { reason } => (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
            )
                .into_response(),
            CompletionHeadStop::Refused { refusal } => {
                crate::adapters::http::verdicts::refusal_response(&refusal)
            }
        }
    }
}

/// Map a completion-head verdict to an early stop, or `None` to proceed. Shared by
/// the three completion handlers (`node_done`, `mark_node_done`, `node_skip`) so
/// the wire shape + log line live once.
///
/// Returns the **typed** stop rather than a `Response`: the reject arm's prose
/// travels in `message` under the `completion_rejected` slug (ADR-0035 §3), which is
/// what lets a client tell "resume the run first" apart from "your outputs are
/// missing" instead of reading every `409` here as an empty `missing_outputs`.
pub fn completion_head_gate(
    head: run_advance::CompletionHead,
    caller: &str,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Option<CompletionHeadStop> {
    match head {
        run_advance::CompletionHead::Reject { reason } => {
            warn!("{caller} rejected for {node_id} iter {iter} in run {run_id}: {reason}");
            Some(CompletionHeadStop::Refused {
                refusal: completion_refusal::CompletionRefusal::CompletionRejected {
                    message: reason,
                },
            })
        }
        run_advance::CompletionHead::NoOp { reason } => {
            info!("{caller} no-op for {node_id} iter {iter} in run {run_id}: {reason}");
            Some(CompletionHeadStop::NoOp { reason })
        }
        run_advance::CompletionHead::Allow => None,
    }
}

/// Who is asking for a node iteration to be completed.
///
/// The **only** thing it changes is the kind of the terminal event appended. Every
/// check, side effect and tail is identical, which is the point: the sweep cannot
/// drift from `POST …/done` because it *is* `POST …/done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    /// The agent called `pdo complete` (`POST /runs/…/nodes/…/done`).
    Explicit,
    /// The liveness sweep observed the end of the agent's turn and turn-end
    /// auto-completion is enabled. Records `NodeAutoCompleted`.
    TurnEnded,
    /// The injected `Stop` hook ran `pdo complete --auto` on turn end — the
    /// *primary*, event-driven substrate (ADR-0043); `TurnEnded` is the daemon-sweep
    /// fallback. Records `NodeAutoCompleted` like `TurnEnded`, differing only in the
    /// log label. The two are idempotent: whichever arrives second gets a `NoOp`.
    StopHook,
    /// The strong binding settled (#724 / ADR-0064): the binding-parked
    /// orchestrator node's watcher saw every child run terminal with no failure
    /// and completed the node itself — « le nœud se complète ». Goes through the
    /// SAME body as every other source (`complete_node_iteration`); the payload
    /// signs WHO decided: the binding, not the agent.
    ChildrenSettled,
}

impl CompletionSource {
    /// The terminal event kind this source records. Both are projected as a
    /// completion and both go through the same completion guard; they differ only
    /// in what the log — and therefore the UI and the stats — will say happened.
    pub fn event_kind(self) -> event::EventKind {
        match self {
            CompletionSource::Explicit => event::EventKind::NodeCompleted,
            CompletionSource::TurnEnded => event::EventKind::NodeAutoCompleted,
            // Reuse the SAME event kind: no new projection / transition-guard arm.
            CompletionSource::StopHook => event::EventKind::NodeAutoCompleted,
            // A truthful completion: the node IS complete, its children settled.
            // `NodeAutoCompleted` would lie about WHO decided — the binding did.
            CompletionSource::ChildrenSettled => event::EventKind::NodeCompleted,
        }
    }

    /// The terminal event's payload. `None` keeps the historical wire shape for
    /// the agent-driven sources; the daemon-driven one signs its verdict, the
    /// same way `mark_node_done` signs `source: "mark_node_done"`.
    pub fn payload(self) -> Option<serde_json::Value> {
        match self {
            Self::ChildrenSettled => Some(serde_json::json!({ "source": "children_settled" })),
            _ => None,
        }
    }

    /// Label for log lines and the aborted-attempt reason.
    pub fn label(self) -> &'static str {
        match self {
            CompletionSource::Explicit => "node_done",
            CompletionSource::TurnEnded => "node_done(auto:turn_ended)",
            CompletionSource::StopHook => "node_done(auto:stop_hook)",
            CompletionSource::ChildrenSettled => "node_done(children_settled)",
        }
    }
}

/// What one pass through [`complete_node_iteration`] did.
///
/// Don't collapse these three: a single "aborted" variant conflated a refusal, a
/// legal no-op and — at the retired merge-resolver site — an outright *success*, which is why
/// "an aborted attempt is never a 2xx" used to be false by construction. `Refused`
/// carries a [`completion_refusal::CompletionRefusal`], a type that owns no status,
/// so the invariant is enforced by the compiler.
pub enum CompletionAttempt {
    /// The terminal event is appended (durable) and the tail is scheduled. The `2xx`
    /// an HTTP caller gets means exactly that, NOT "advanced" (ADR-0023).
    Completed,
    /// Valid, but nothing to do: a legal duplicate on an already-terminal node. The
    /// **only** non-`Completed` variant allowed a `2xx` — a puzzled agent re-running
    /// `pdo complete` must not read "refused" and chain `pdo fail`, which on a
    /// `script` node its tail would do unprompted.
    NoOp { reason: String },
    /// The completion is not granted. `Refused ⇒ never 2xx`, by construction: the
    /// carried refusal has no status of its own, and `refusal_response` is the only
    /// thing that gives it one.
    Refused {
        refusal: completion_refusal::CompletionRefusal,
    },
}

impl CompletionAttempt {
    /// Sugar for the many early returns of the shared body.
    pub fn refused(refusal: completion_refusal::CompletionRefusal) -> Self {
        CompletionAttempt::Refused { refusal }
    }

    /// The log-friendly cause, for the liveness sweep — which reads the *variant*,
    /// never the status.
    pub fn reason(&self) -> String {
        match self {
            CompletionAttempt::Completed => "completed".into(),
            CompletionAttempt::NoOp { reason } => reason.clone(),
            CompletionAttempt::Refused { refusal } => refusal.reason(),
        }
    }

    /// Collapse back to the HTTP response `POST …/done` answers with.
    pub fn into_response(self) -> Response {
        match self {
            CompletionAttempt::Completed => (StatusCode::OK, "ok").into_response(),
            CompletionAttempt::NoOp { reason } => (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
            )
                .into_response(),
            CompletionAttempt::Refused { refusal } => {
                crate::adapters::http::verdicts::refusal_response(&refusal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    #[test]
    fn stop_hook_source_reuses_auto_completed_with_a_distinct_label() {
        // #433 / ADR-0043: the hook completion is AUTOMATIC — it must record the
        // SAME `NodeAutoCompleted` event as the daemon sweep (no new EventKind, no
        // projection/transition-guard arm, no ADR-0032 regression), and only its
        // runtime log label distinguishes hook from sweep.
        assert_eq!(
            CompletionSource::StopHook.event_kind(),
            event_log::EventKind::NodeAutoCompleted
        );
        assert_eq!(
            CompletionSource::TurnEnded.event_kind(),
            CompletionSource::StopHook.event_kind(),
            "hook and sweep record the same kind"
        );
        assert_eq!(
            CompletionSource::StopHook.label(),
            "node_done(auto:stop_hook)"
        );
        assert_ne!(
            CompletionSource::StopHook.label(),
            CompletionSource::TurnEnded.label(),
            "the labels must stay distinguishable in the runtime log"
        );
        // The explicit (agent-typed) path is unchanged.
        assert_eq!(
            CompletionSource::Explicit.event_kind(),
            event_log::EventKind::NodeCompleted
        );
    }

    /// The three `CompletionAttempt` variants, behind a `match` with **no wildcard**:
    /// a fourth variant does not compile until its status side is decided.
    ///
    /// This is the half of the invariant that lives outside `completion_refusal.rs`:
    /// `NoOp` is the ONE non-`Completed` outcome allowed a `2xx`, because a legal
    /// duplicate grants nothing and must not read as an error (ADR-0025 §3).
    #[test]
    fn noop_is_the_only_2xx_that_grants_nothing() {
        for attempt in [
            CompletionAttempt::Completed,
            CompletionAttempt::NoOp {
                reason: "already terminal".into(),
            },
            CompletionAttempt::refused(completion_refusal::CompletionRefusal::MissingOutputs {
                missing: vec!["review".into()],
            }),
        ] {
            let expect_2xx = match &attempt {
                CompletionAttempt::Completed | CompletionAttempt::NoOp { .. } => true,
                CompletionAttempt::Refused { .. } => false,
            };
            let status = attempt.into_response().status();
            assert_eq!(
                status.is_success(),
                expect_2xx,
                "wrong status class: {status}"
            );
        }
    }
}
