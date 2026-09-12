//! Harness-specific **steering message** counting (#790/#791/#792, Stats →
//! Performance's third metric).
//!
//! A *steering message* (CONTEXT.md « Message de pilotage ») is a turn a human
//! typed into a Node's session **after** its launch. The counter is derived at
//! read time from the harness transcript (ADR-0029: never persisted), one parser
//! per harness (ADR-0045/0051: a capability is code, written harness by harness,
//! dispatched through [`crate::harness_probes`]).
//!
//! Three rules shared by every parser, applied here so no harness drifts:
//!
//! 1. **The launch prompt is excluded by position**: the first textual human
//!    turn of the main session file is the prompt PDO pasted at spawn. A resume
//!    (`--resume`) appends to the same file, a `restart_node` opens a new one —
//!    the rule holds either way.
//! 2. **Runtime messages are excluded by prefix**: every text the daemon itself
//!    types or pastes into a harness session starts with [`RUNTIME_PREFIX`],
//!    posed once at the tmux bottleneck
//!    ([`crate::tmux_session_manager::send_message`] /
//!    [`crate::tmux_session_manager::paste_message`]). A turn starting with it
//!    is PDO's, not a human's.
//! 3. **Only typed text counts**: tool results, meta/skill loads, compaction
//!    summaries, interruptions and local-command echoes are never a steering
//!    message — the number must match what the human actually typed.
//!
//! The result is `Some(n)` for a readable transcript (a session with no human
//! message after its launch is `Some(0)` — an *autonomous* execution, which the
//! steered rate must count honestly), `None` when no human turn can be read at
//! all (an empty or foreign file), so an unreadable transcript is never mistaken
//! for zero steering.

use serde_json::Value;

/// The marker every daemon-authored message carries in a harness session, at
/// the head of its first line. Posed by the tmux session manager, subtracted by
/// the counters below; callers on either side never spell it.
pub const RUNTIME_PREFIX: &str = "[pdo-runtime]";

/// Prefix `text` as a runtime message — idempotent, so a caller that already
/// did it (there should be none) never double-tags.
pub fn runtime_message(text: &str) -> String {
    if text.starts_with(RUNTIME_PREFIX) {
        text.to_string()
    } else {
        format!("{RUNTIME_PREFIX} {text}")
    }
}

fn is_runtime_message(text: &str) -> bool {
    text.trim_start().starts_with(RUNTIME_PREFIX)
}

/// The typed text of a message `content` — a plain string, or the `text` blocks
/// of a block array (claude / pi). `None` when the content carries no text at
/// all, or contains a `tool_result` block (a tool round-trip, never a human).
fn typed_text(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => (!s.trim().is_empty()).then(|| s.clone()),
        Value::Array(blocks) => {
            let mut text = String::new();
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("tool_result") => return None,
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                        }
                    }
                    _ => {}
                }
            }
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

/// Text a harness writes as a user turn on the human's behalf, without the
/// human typing it: an interruption marker, a local slash-command echo.
fn is_synthetic_user_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("[Request interrupted by user")
        || t.starts_with("<local-command-stdout>")
        || t.starts_with("<local-command-caveat>")
}

/// Count the steering messages in an ordered list of human turns: drop the
/// first (the launch prompt), keep the rest minus the runtime-prefixed ones.
fn count_after_launch(turns: Vec<String>) -> Option<u32> {
    if turns.is_empty() {
        return None;
    }
    Some(
        turns
            .into_iter()
            .skip(1)
            .filter(|t| !is_runtime_message(t))
            .count() as u32,
    )
}

/// `claude`: `type:"user"` lines with typed text of the **main** session file,
/// minus the first, minus `isMeta` (skill loads, caveats), `toolUseResult` /
/// `tool_result` (tool round-trips), `isCompactSummary` (compaction replays),
/// sidechain lines (a subagent's turns in a pre-split transcript) and runtime-
/// prefixed turns. Subagent files are never handed here: they are never steered.
pub(crate) fn claude_steering_count(text: &str) -> Option<u32> {
    let mut turns = Vec::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let flagged = |key: &str| v.get(key).and_then(Value::as_bool) == Some(true);
        if flagged("isMeta") || flagged("isCompactSummary") || flagged("isSidechain") {
            continue;
        }
        if v.get("toolUseResult").is_some() {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if role != "user" {
                continue;
            }
        }
        let Some(content) = msg.get("content").and_then(typed_text) else {
            continue;
        };
        if is_synthetic_user_text(&content) {
            continue;
        }
        turns.push(content);
    }
    count_after_launch(turns)
}

/// `copilot`: `user.message` events of the session journal, minus the first,
/// minus runtime-prefixed ones.
pub(crate) fn copilot_steering_count(text: &str) -> Option<u32> {
    let mut turns = Vec::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user.message") {
            continue;
        }
        let Some(content) = v
            .get("data")
            .and_then(|d| d.get("content"))
            .and_then(typed_text)
        else {
            continue;
        };
        turns.push(content);
    }
    count_after_launch(turns)
}

/// `pi`: `type:"message"` entries of role `user` with typed text, minus the
/// first, minus runtime-prefixed ones.
pub(crate) fn pi_steering_count(text: &str) -> Option<u32> {
    let mut turns = Vec::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("message") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let Some(content) = msg.get("content").and_then(typed_text) else {
            continue;
        };
        turns.push(content);
    }
    count_after_launch(turns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_user(text: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":{}}}}}"#,
            Value::from(text)
        )
    }

    #[test]
    fn runtime_message_prefixes_once() {
        assert_eq!(runtime_message("hello"), "[pdo-runtime] hello");
        assert_eq!(
            runtime_message("[pdo-runtime] hello"),
            "[pdo-runtime] hello"
        );
    }

    #[test]
    fn claude_excludes_the_launch_prompt_by_position() {
        let text = [
            claude_user("launch"),
            claude_user("steer 1"),
            claude_user("steer 2"),
        ]
        .join("\n");
        assert_eq!(claude_steering_count(&text), Some(2));
    }

    #[test]
    fn claude_a_lone_launch_prompt_is_zero_steering_not_unreadable() {
        assert_eq!(claude_steering_count(&claude_user("launch")), Some(0));
        assert_eq!(claude_steering_count(""), None);
        assert_eq!(
            claude_steering_count(r#"{"type":"assistant","message":{"model":"m","usage":{}}}"#),
            None
        );
    }

    #[test]
    fn claude_subtracts_runtime_tagged_turns() {
        let text = [
            claude_user("launch"),
            claude_user("[pdo-runtime] your output is missing frontmatter"),
            claude_user("human steer"),
        ]
        .join("\n");
        assert_eq!(claude_steering_count(&text), Some(1));
    }

    #[test]
    fn claude_ignores_tool_results_meta_compaction_sidechain_and_interrupts() {
        let text = [
            claude_user("launch"),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"out"}]},"toolUseResult":{"stdout":"out"}}"#.to_string(),
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: /x"}]}}"#.to_string(),
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"This session is being continued from a previous conversation"}}"#.to_string(),
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent prompt"}}"#.to_string(),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#.to_string(),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"typed as a block"}]}}"#.to_string(),
            "not json".to_string(),
        ]
        .join("\n");
        assert_eq!(claude_steering_count(&text), Some(1));
    }

    #[test]
    fn copilot_counts_user_message_events_after_the_first() {
        let text = [
            r#"{"type":"session.start","data":{}}"#,
            r#"{"type":"user.message","data":{"content":"launch"}}"#,
            r#"{"type":"assistant.turn_end","data":{}}"#,
            r#"{"type":"user.message","data":{"content":"[pdo-runtime] retry"}}"#,
            r#"{"type":"user.message","data":{"content":"steer"}}"#,
            r#"{"type":"user.message","data":{"content":"steer again"}}"#,
        ]
        .join("\n");
        assert_eq!(copilot_steering_count(&text), Some(2));
        assert_eq!(
            copilot_steering_count(r#"{"type":"session.start","data":{}}"#),
            None
        );
    }

    #[test]
    fn pi_counts_user_role_messages_after_the_first() {
        let text = [
            r#"{"type":"session","id":"s"}"#,
            r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"launch"}]}}"#,
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"message","message":{"role":"user","content":"steer"}}"#,
            r#"{"type":"message","message":{"role":"user","content":"[pdo-runtime] retry"}}"#,
            r#"{"type":"message","message":{"role":"toolResult","content":[{"type":"text","text":"x"}]}}"#,
        ]
        .join("\n");
        assert_eq!(pi_steering_count(&text), Some(1));
        assert_eq!(pi_steering_count(r#"{"type":"session","id":"s"}"#), None);
    }
}
