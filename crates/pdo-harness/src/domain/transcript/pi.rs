//! `pi`'s session lines: `type:"message"` entries of role `user`, `assistant`
//! (blocks `text`, `thinking`, `toolCall`) and `toolResult`.

use serde_json::Value;

use super::{str_field, text_of, Speaker, TranscriptRecord};

/// The records of one pi session line. `thinking` blocks are never read; the
/// entry `id` dedups a replayed message.
pub fn records(line: &str) -> Vec<TranscriptRecord> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    if v.get("type").and_then(Value::as_str) != Some("message") {
        return Vec::new();
    }
    let Some(message) = v.get("message") else {
        return Vec::new();
    };
    let at = str_field(&v, "timestamp");
    let key = str_field(&v, "id");
    let content = message.get("content").unwrap_or(&Value::Null);
    match message.get("role").and_then(Value::as_str) {
        Some("user") => vec![TranscriptRecord::Message {
            speaker: Speaker::Prompt,
            text: text_of(content),
            at,
            key,
        }],
        Some("assistant") => {
            let mut out = vec![TranscriptRecord::Message {
                speaker: Speaker::Agent,
                text: text_of(content),
                at: at.clone(),
                key,
            }];
            for block in content.as_array().into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) != Some("toolCall") {
                    continue;
                }
                let Some(call_id) = str_field(block, "id") else {
                    continue;
                };
                out.push(TranscriptRecord::ToolCall {
                    call_id,
                    name: str_field(block, "name").unwrap_or_default(),
                    input: block.get("arguments").cloned().unwrap_or(Value::Null),
                    at: at.clone(),
                });
            }
            out
        }
        Some("toolResult") => {
            let Some(call_id) = str_field(message, "toolCallId") else {
                return Vec::new();
            };
            vec![TranscriptRecord::ToolResult {
                call_id,
                output: text_of(content),
                is_error: message.get("isError").and_then(Value::as_bool) == Some(true),
            }]
        }
        _ => Vec::new(),
    }
}
