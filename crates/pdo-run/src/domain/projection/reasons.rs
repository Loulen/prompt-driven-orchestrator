//! The reason and the machine reason code a park, a give-up or a non-green
//! terminal carries.

use crate::domain::event::Event;

/// The `reason` a non-green run terminal carries, if it says anything (#503).
///
/// An empty string is treated as absent: `Some("")` in the projection would make
/// the UI render an explanation box with nothing in it, which is worse than the
/// red dot it replaces.
pub(super) fn run_event_reason(event: &Event) -> Option<String> {
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
pub fn interrupt_payload(code: &str, reason: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "reason_code": code, "reason": reason.into() })
}

/// Extract the machine slug a park/give-up prose reason is prefixed with (#601).
/// By convention every interrupt reason reads `"<slug>: <prose>"` (e.g.
/// `"session_died: tmux session … gone"`); the slug is the machine-branchable
/// code, the whole string stays the human sentence. Returns the slug when the
/// head before the first `": "` is a non-empty `snake_case` token (lowercase,
/// digits, `_`) and prose follows — else `None` (an un-prefixed legacy reason
/// carries no derivable code).
pub fn parse_reason_code(reason: &str) -> Option<String> {
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
pub(super) fn run_event_reason_code(event: &Event) -> Option<String> {
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
