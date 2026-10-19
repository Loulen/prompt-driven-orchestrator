//! `copilot`'s event journal lines: `user.message`, `assistant.message` (its
//! `content`, its `toolRequests`) and `tool.execution_complete`.

use serde_json::Value;

use super::{str_field, Speaker, TranscriptRecord};

/// The records of one Copilot CLI journal event.
///
/// The `reasoningText` / `reasoningOpaque` / `encryptedContent` of an
/// assistant message are its thinking: never read. `system.message` (the
/// harness's system prompt) is not a turn of the conversation. The event `id`
/// dedups a replayed message.
pub fn records(line: &str) -> Vec<TranscriptRecord> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let Some(data) = v.get("data") else {
        return Vec::new();
    };
    let at = str_field(&v, "timestamp");
    let key = str_field(&v, "id");
    match v.get("type").and_then(Value::as_str) {
        Some("user.message") => vec![TranscriptRecord::Message {
            speaker: Speaker::Prompt,
            text: str_field(data, "content").unwrap_or_default(),
            at,
            key,
        }],
        Some("assistant.message") => {
            let mut out = vec![TranscriptRecord::Message {
                speaker: Speaker::Agent,
                text: str_field(data, "content").unwrap_or_default(),
                at: at.clone(),
                key,
            }];
            for request in data
                .get("toolRequests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(call_id) = str_field(request, "toolCallId") else {
                    continue;
                };
                out.push(TranscriptRecord::ToolCall {
                    call_id,
                    name: str_field(request, "name").unwrap_or_default(),
                    input: request.get("arguments").cloned().unwrap_or(Value::Null),
                    at: at.clone(),
                });
            }
            out
        }
        Some("tool.execution_complete") => {
            let Some(call_id) = str_field(data, "toolCallId") else {
                return Vec::new();
            };
            let success = data.get("success").and_then(Value::as_bool) != Some(false);
            let output = data
                .get("result")
                .and_then(|r| r.get("content"))
                .and_then(Value::as_str)
                .or_else(|| {
                    data.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default()
                .to_string();
            vec![TranscriptRecord::ToolResult {
                call_id,
                output,
                is_error: !success,
            }]
        }
        _ => Vec::new(),
    }
}
