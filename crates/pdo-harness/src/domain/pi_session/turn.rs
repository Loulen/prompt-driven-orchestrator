//! The sweep's fallback end-of-turn classifier on a pi session tail, and the
//! hard error pi survives.

use super::*;

/// What the last substantial record of a tail was, for [`turn_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastRecord {
    /// An assistant message that ended its turn (`stop` / `length`).
    AssistantSettled,
    /// An assistant message that did not settle: `error`, `aborted`, or a
    /// `toolUse` whose results all came back (the next call is pending).
    AssistantUnsettled,
    /// A user prompt or a tool result: the assistant owes a reply.
    User,
}

/// Classify the tail of a pi session JSONL into a [`TurnState`] (the sweep's
/// fallback substrate, CONTEXT.md § "Extension de fin de tour"). Pure — the caller
/// reads the tail.
///
/// One forward pass over the lines:
/// 1. every `toolCall` block of an assistant message opens its `id`;
/// 2. every `toolResult` message closes its `toolCallId`;
/// 3. an id still open at the end ⇒ [`TurnState::InToolCall`], **checked first**
///    (the record that opened it is itself an assistant message);
/// 4. otherwise the last substantial record decides: an assistant `stop` /
///    `length` ⇒ [`TurnState::TurnEnded`]; a user prompt or a tool result ⇒
///    [`TurnState::AwaitingAssistant`]; an assistant `error` / `aborted` ⇒
///    [`TurnState::Unknown`] — not a finished turn, and not one the sweep should
///    touch (the hard error is said by [`hard_error`], the abort is the human's).
///
/// Unparseable lines are skipped, which is what makes a byte-clipped tail safe. A
/// `toolResult` whose `toolCall` fell outside the window closes nothing (harmless);
/// a `toolCall` whose result fell outside stays open (conservative).
pub fn turn_state(tail: &str) -> TurnState {
    let mut open_calls: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last: Option<LastRecord> = None;
    for raw in tail.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("message") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        match msg.get("role").and_then(|r| r.as_str()) {
            Some("assistant") => {
                if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                    for b in blocks {
                        if b.get("type").and_then(|t| t.as_str()) == Some("toolCall") {
                            if let Some(id) = b.get("id").and_then(|i| i.as_str()) {
                                open_calls.insert(id.to_string());
                            }
                        }
                    }
                }
                last = Some(match msg.get("stopReason").and_then(|s| s.as_str()) {
                    Some("stop") | Some("length") => LastRecord::AssistantSettled,
                    _ => LastRecord::AssistantUnsettled,
                });
            }
            Some("toolResult") => {
                if let Some(id) = msg.get("toolCallId").and_then(|i| i.as_str()) {
                    open_calls.remove(id);
                }
                last = Some(LastRecord::User);
            }
            Some("user") => last = Some(LastRecord::User),
            _ => {}
        }
    }
    if !open_calls.is_empty() {
        return TurnState::InToolCall;
    }
    match last {
        Some(LastRecord::AssistantSettled) => TurnState::TurnEnded,
        Some(LastRecord::User) => TurnState::AwaitingAssistant,
        Some(LastRecord::AssistantUnsettled) | None => TurnState::Unknown,
    }
}

/// Whether this session `tail` shows a **finished turn** — pi's end-of-turn
/// signature for the sweep's fallback. A trailing `error` is never a finished turn,
/// so an errored node is not auto-completed as if it had succeeded.
pub fn turn_ended(tail: &str) -> bool {
    turn_state(tail) == TurnState::TurnEnded
}

/// Whether this session `tail` trails on a **hard error** — an assistant message
/// whose `stopReason` is `error`, with its `errorMessage` (best-effort). This is the
/// failure pi's exit code cannot report: the process stays resident after it
/// (`exit_code_is_verdict` is `false` for pi), so the text is the verdict. A later
/// successful assistant message clears it (pi auto-retries).
pub fn hard_error(tail: &str) -> Option<String> {
    let mut last_error: Option<Option<String>> = None;
    for raw in tail.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("message") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        if msg.get("stopReason").and_then(|s| s.as_str()) == Some("error") {
            last_error = Some(
                msg.get("errorMessage")
                    .and_then(|m| m.as_str())
                    .map(String::from),
            );
        } else {
            last_error = None;
        }
    }
    last_error.map(|m| m.unwrap_or_else(|| "pi assistant error".to_string()))
}
