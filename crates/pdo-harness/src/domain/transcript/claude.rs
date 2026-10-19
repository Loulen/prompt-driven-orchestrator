//! `claude`'s transcript lines: `type:"user"` / `type:"assistant"` records
//! whose `message.content` is a string or an array of blocks (`text`,
//! `thinking`, `tool_use`, `tool_result`).

use serde_json::Value;

use super::{str_field, text_of, Speaker, TranscriptRecord};

/// The records of one Claude Code transcript line.
///
/// Skipped: metadata lines (no `message`), `isMeta` user turns (skill loads,
/// caveats), compaction summaries, sidechain lines (a subagent's turns in a
/// pre-split transcript), and every `thinking` / `redacted_thinking` block.
/// A line's `uuid` dedups a replayed message.
pub fn records(line: &str) -> Vec<TranscriptRecord> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let kind = v.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("user") | Some("assistant")) {
        return Vec::new();
    }
    let flagged = |key: &str| v.get(key).and_then(Value::as_bool) == Some(true);
    if flagged("isSidechain") || flagged("isMeta") || flagged("isCompactSummary") {
        return Vec::new();
    }
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return Vec::new();
    };
    let at = str_field(&v, "timestamp");
    let key = str_field(&v, "uuid");
    let speaker = if kind == Some("assistant") {
        Speaker::Agent
    } else {
        Speaker::Prompt
    };

    let mut out = Vec::new();
    let mut text = String::new();
    match content {
        Value::String(s) => text.push_str(s),
        Value::Array(blocks) => {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                    }
                    Some("tool_use") if speaker == Speaker::Agent => {
                        let Some(call_id) = str_field(block, "id") else {
                            continue;
                        };
                        out.push(TranscriptRecord::ToolCall {
                            call_id,
                            name: str_field(block, "name").unwrap_or_default(),
                            input: block.get("input").cloned().unwrap_or(Value::Null),
                            at: at.clone(),
                        });
                    }
                    Some("tool_result") => {
                        let Some(call_id) = str_field(block, "tool_use_id") else {
                            continue;
                        };
                        out.push(TranscriptRecord::ToolResult {
                            call_id,
                            output: block.get("content").map(text_of).unwrap_or_default(),
                            is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
                        });
                    }
                    // `thinking`, `redacted_thinking`, images: never shown.
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if !text.trim().is_empty() {
        out.insert(
            0,
            TranscriptRecord::Message {
                speaker,
                text,
                at,
                key,
            },
        );
    }
    out
}
