//! The **declared wait** (#588 / ADR-0069 §1): a node is `awaiting_user` when its
//! agent says so — never inferred from a spawn or from a silence.
//!
//! Three doors into the wait, one out by keystroke:
//! - `pdo wait-user [--message "<question>"]` → [`declare_wait`] with cause
//!   `declared`; accepted on **any** node holding a live session, interactive or
//!   not (a node stuck on a question becomes visible, #290); refused by name on a
//!   `script` node or without a live session; idempotent (a repeat with the same
//!   message is a `noop`, a new message refreshes the banner).
//! - a refused `pdo complete` on an unreleased interactive node → the daemon
//!   declares the wait for the agent, cause `completion_not_released`.
//! - the orchestrator binding marker (ADR-0064) keeps its own `children_pending`.
//! - **Out**: an Enter key that crossed the UI's PTY bridge after at least one
//!   typed byte since the declaration ([`note_pty_input`]) appends `NodeResumed`;
//!   the completion release (ADR-0068) keeps its own event. Arrow keys, scroll,
//!   mouse reports and the runtime's own `paste_text` never lift a wait — the
//!   first two are escape sequences, the last never crosses the bridge.

use std::sync::Arc;

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use tracing::{info, warn};

use crate::event_log::{
    self, NodeStatus, AWAITING_CAUSE_COMPLETION_NOT_RELEASED, AWAITING_CAUSE_DECLARED,
};
use crate::{append_event, load_events, tmux_session_manager, AppState};

/// `--message` is capped so the banner never grows past one line (design
/// decision, 2026-09-12). Both the CLI and the daemon truncate — the daemon
/// defensively, for a client that is not `pdo`.
pub(crate) const MESSAGE_MAX_CHARS: usize = 100;

/// Truncate a wait message to [`MESSAGE_MAX_CHARS`] characters, with an ellipsis.
/// Whitespace is collapsed to one line first: the banner has one line to give.
pub(crate) fn truncate_message(raw: &str) -> String {
    let one_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MESSAGE_MAX_CHARS {
        return one_line;
    }
    let mut out: String = one_line.chars().take(MESSAGE_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// The daemon's sentence when a refused `pdo complete` declares the wait. The
/// banner (A3) renders it with the two buttons as inline links.
pub(crate) const COMPLETION_NOT_RELEASED_MESSAGE: &str =
    "Completion not released. Click Mark ready for completion so the agent can finish, or Mark complete to take the artifacts as they are.";

/// Why a wait could not be declared. Every variant is a named `409`; the CLI
/// maps them to exit 3 (refused, still your turn — nothing terminal happened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WaitRefusal {
    RunNotFound,
    RunNotLive,
    NodeNotFound,
    /// A `script` node runs no agent: nobody there can be waiting on a human.
    NodeIsScript,
    NodeSessionNotLive,
}

impl WaitRefusal {
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Self::RunNotFound => "run_not_found",
            Self::RunNotLive => "run_not_live",
            Self::NodeNotFound => "node_not_found",
            Self::NodeIsScript => "node_is_script",
            Self::NodeSessionNotLive => "node_session_not_live",
        }
    }

    fn message(&self, run_id: &str, node_id: &str, iter: i64) -> String {
        match self {
            Self::RunNotFound => format!("run {run_id} does not exist"),
            Self::RunNotLive => format!("run {run_id} has no live session to wait on"),
            Self::NodeNotFound => format!("node {node_id} is not part of run {run_id}"),
            Self::NodeIsScript => {
                format!("node {node_id} is a script node: it runs no agent that could wait on you")
            }
            Self::NodeSessionNotLive => {
                format!("node {node_id} iter {iter} has no live session")
            }
        }
    }

    pub(crate) fn into_response(self, run_id: &str, node_id: &str, iter: i64) -> Response {
        let status = match self {
            Self::RunNotFound | Self::NodeNotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::CONFLICT,
        };
        (
            status,
            Json(serde_json::json!({
                "error": self.slug(),
                "recoverable": true,
                "message": self.message(run_id, node_id, iter),
            })),
        )
            .into_response()
    }
}

/// What one declaration did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclareOutcome {
    /// The node was running: it is now awaiting, with this message.
    Declared,
    /// Already awaiting with the same cause and message: nothing written.
    Noop,
    /// Already awaiting; the message (or cause) changed, so the banner refreshes.
    Refreshed,
}

/// Declare the wait of `(run, node, iter)`. Appends one `NodeAwaitingUser` with
/// `{cause, message}` unless the node already awaits on exactly that, and
/// forgets any keystroke typed before the declaration (the Enter rule counts
/// from here).
pub(crate) async fn declare_wait(
    state: &Arc<AppState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
    cause: &str,
    message: Option<&str>,
    session_alive: impl FnOnce(&str) -> bool,
) -> Result<DeclareOutcome, WaitRefusal> {
    let events = load_events(&state.db, run_id)
        .await
        .map_err(|_| WaitRefusal::RunNotFound)?;
    let Some(run_state) = event_log::project(&events) else {
        return Err(WaitRefusal::RunNotFound);
    };
    if !run_state.status.is_live() {
        return Err(WaitRefusal::RunNotLive);
    }
    let Some(node) = run_state.nodes.get(node_id) else {
        // A defined-but-never-started node has no session either; say which.
        return Err(if run_state.node_defs.iter().any(|d| d.id == node_id) {
            WaitRefusal::NodeSessionNotLive
        } else {
            WaitRefusal::NodeNotFound
        });
    };
    if run_state
        .node_defs
        .iter()
        .any(|d| d.id == node_id && d.node_type == "script")
    {
        return Err(WaitRefusal::NodeIsScript);
    }
    let Some(attempt) = node.iterations.iter().find(|a| a.iter == iter) else {
        return Err(WaitRefusal::NodeSessionNotLive);
    };
    if !attempt.status.holds_session() || iter != node.iter {
        return Err(WaitRefusal::NodeSessionNotLive);
    }
    // The projection says the node holds a session; tmux has the last word. A
    // pane that died before the sweep noticed must not park the run on a
    // question nobody can answer in it (FP #793, finding 3).
    let session = tmux_session_manager::node_session_name(run_id, node_id, iter);
    if !session_alive(&session) {
        return Err(WaitRefusal::NodeSessionNotLive);
    }

    // The cap is for the agent's `--message`; the daemon's own sentence on a
    // refused completion is authored to fit the banner and stays whole.
    let message = message
        .map(|m| {
            if cause == AWAITING_CAUSE_DECLARED {
                truncate_message(m)
            } else {
                m.trim().to_string()
            }
        })
        .filter(|m| !m.is_empty());
    let already = node.status == NodeStatus::AwaitingUser;
    if already {
        if let Some(current) = node.awaiting.as_ref() {
            if current.cause == cause && current.message == message {
                return Ok(DeclareOutcome::Noop);
            }
        }
    }

    let mut payload = serde_json::json!({ "cause": cause });
    if let Some(m) = &message {
        payload["message"] = serde_json::json!(m);
    }
    let event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeAwaitingUser,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(payload),
    };
    if let Err(e) = append_event(state, &event).await {
        warn!("declared wait: failed to append NodeAwaitingUser for {node_id} in {run_id}: {e}");
        return Err(WaitRefusal::RunNotLive);
    }
    // The Enter rule counts typed bytes from the declaration on.
    state.pty_typed.lock().unwrap().remove(&session);
    info!(
        "Run {run_id}: node {node_id} iter-{iter} declared a wait ({cause}{})",
        message
            .as_deref()
            .map(|m| format!(": {m}"))
            .unwrap_or_default()
    );
    Ok(if already {
        DeclareOutcome::Refreshed
    } else {
        DeclareOutcome::Declared
    })
}

/// The daemon-side declaration on a refused unreleased completion (ADR-0069 §1):
/// best-effort, never changes the refusal itself.
pub(crate) async fn declare_completion_not_released(
    state: &Arc<AppState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) {
    if let Err(refusal) = declare_wait(
        state,
        run_id,
        node_id,
        iter,
        AWAITING_CAUSE_COMPLETION_NOT_RELEASED,
        Some(COMPLETION_NOT_RELEASED_MESSAGE),
        |session| state.node_session_alive(session),
    )
    .await
    {
        warn!(
            "Run {run_id}: refused completion of {node_id} could not declare the wait ({})",
            refusal.slug()
        );
    }
}

/// What a chunk of PTY input from the browser amounts to, for the Enter rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InputSignal {
    /// At least one **typed** byte: printable ASCII or a UTF-8 lead/continuation
    /// byte, outside any escape sequence. Arrow keys, mouse reports and scroll
    /// (all `ESC [ …`), Backspace, Tab and bare control bytes do not count.
    pub(crate) typed: bool,
    /// At least one Enter (`\r`, or `\n` for a client that sends it) outside any
    /// escape sequence.
    pub(crate) enter: bool,
}

/// Classify one binary frame from the PTY WebSocket. Escape sequences are
/// skipped whole: `ESC [ … <final 0x40..=0x7e>` (CSI, which covers arrows, mouse
/// reports and bracketed-paste markers) and `ESC O <byte>` (SS3 keypad), plus a
/// bare `ESC <byte>`.
pub(crate) fn classify_input(bytes: &[u8]) -> InputSignal {
    let mut signal = InputSignal::default();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            i += 1;
            match bytes.get(i) {
                Some(b'[') => {
                    i += 1;
                    while let Some(&c) = bytes.get(i) {
                        i += 1;
                        if (0x40..=0x7e).contains(&c) {
                            break;
                        }
                    }
                }
                Some(b'O') => i += 2,
                Some(_) => i += 1,
                None => {}
            }
            continue;
        }
        if b == b'\r' || b == b'\n' {
            signal.enter = true;
        } else if (0x20..=0x7e).contains(&b) || b >= 0x80 {
            signal.typed = true;
        }
        i += 1;
    }
    signal
}

/// Called by the PTY bridge for every binary frame a browser writes into a
/// **node** session. Remembers typed bytes per session; on an Enter that follows
/// at least one typed byte since the last declaration, lifts the wait if the
/// node is awaiting on a declared / refused-completion cause. Never touches a
/// `children_pending` marker (the orchestrator gave its turn back, ADR-0064) nor
/// a derived wait (nothing is written on the parent).
pub(crate) async fn note_pty_input(state: &Arc<AppState>, session_name: &str, bytes: &[u8]) {
    let Some(tmux_session_manager::ParsedSession::NodeRun {
        run_id,
        node_id,
        iter,
    }) = tmux_session_manager::parse_session_name(session_name)
    else {
        return;
    };
    let signal = classify_input(bytes);
    if !signal.typed && !signal.enter {
        return;
    }
    let lift = {
        let mut typed = state.pty_typed.lock().unwrap();
        if signal.typed {
            typed.insert(session_name.to_string());
        }
        if signal.enter && typed.contains(session_name) {
            typed.remove(session_name);
            true
        } else {
            false
        }
    };
    if !lift {
        return;
    }
    if let Err(e) = lift_wait(state, &run_id, &node_id, iter).await {
        warn!("Run {run_id}: failed to lift the wait of {node_id} iter-{iter}: {e}");
    }
}

/// Append `NodeResumed` if the node awaits on a cause a keystroke may lift.
/// Returns whether anything was written.
pub(crate) async fn lift_wait(
    state: &Arc<AppState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> anyhow::Result<bool> {
    let events = load_events(&state.db, run_id).await?;
    let Some(run_state) = event_log::project(&events) else {
        return Ok(false);
    };
    let Some(node) = run_state.nodes.get(node_id) else {
        return Ok(false);
    };
    if node.iter != iter || node.status != NodeStatus::AwaitingUser {
        return Ok(false);
    }
    let liftable = node.awaiting.as_ref().is_none_or(|a| {
        a.cause == AWAITING_CAUSE_DECLARED || a.cause == AWAITING_CAUSE_COMPLETION_NOT_RELEASED
    });
    if !liftable {
        return Ok(false);
    }
    let event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeResumed,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "source": "pty_enter" })),
    };
    append_event(state, &event).await?;
    info!("Run {run_id}: node {node_id} iter-{iter} resumed on the user's Enter");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn message_is_collapsed_to_one_line_and_capped_with_an_ellipsis() {
        assert_eq!(truncate_message("  a\n b  "), "a b");
        let long = "x".repeat(150);
        let capped = truncate_message(&long);
        assert_eq!(capped.chars().count(), MESSAGE_MAX_CHARS);
        assert!(capped.ends_with('…'));
        let exact = "y".repeat(MESSAGE_MAX_CHARS);
        assert_eq!(truncate_message(&exact), exact);
    }

    #[test]
    fn typed_text_then_enter_is_both() {
        let s = classify_input(b"the strip\r");
        assert_eq!(
            s,
            InputSignal {
                typed: true,
                enter: true
            }
        );
    }

    #[test]
    fn a_bare_enter_is_not_typed() {
        assert_eq!(
            classify_input(b"\r"),
            InputSignal {
                typed: false,
                enter: true
            }
        );
        assert_eq!(
            classify_input(b"\n"),
            InputSignal {
                typed: false,
                enter: true
            }
        );
    }

    #[test]
    fn arrows_mouse_and_scroll_are_neither() {
        // Up arrow, left arrow (application mode), SGR mouse wheel report.
        for seq in [
            &b"\x1b[A"[..],
            b"\x1bOD",
            b"\x1b[<64;10;5M",
            b"\x1b[<65;10;5M",
        ] {
            assert_eq!(classify_input(seq), InputSignal::default(), "{seq:?}");
        }
    }

    #[test]
    fn backspace_tab_and_control_bytes_are_not_typed() {
        assert_eq!(classify_input(b"\x7f\t\x03"), InputSignal::default());
    }

    #[test]
    fn utf8_counts_as_typed() {
        assert_eq!(
            classify_input("é".as_bytes()),
            InputSignal {
                typed: true,
                enter: false
            }
        );
    }

    #[test]
    fn bracketed_paste_text_counts_as_typed() {
        let s = classify_input(b"\x1b[200~pasted answer\x1b[201~");
        assert_eq!(
            s,
            InputSignal {
                typed: true,
                enter: false
            }
        );
    }

    #[test]
    fn refusal_slugs_are_stable() {
        assert_eq!(WaitRefusal::NodeIsScript.slug(), "node_is_script");
        assert_eq!(
            WaitRefusal::NodeSessionNotLive.slug(),
            "node_session_not_live"
        );
        assert_eq!(WaitRefusal::RunNotLive.slug(), "run_not_live");
    }
}
