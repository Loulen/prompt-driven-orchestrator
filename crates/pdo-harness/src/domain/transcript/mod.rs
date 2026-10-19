//! The **normalized transcript** of a session (CONTEXT.md « Transcript »): what
//! a node's Transcript tab shows, read from the harness's own session file and
//! the same for every harness that declares a reader.
//!
//! Three layers, all pure (bytes in, facts out, no I/O):
//!
//! - **records** — one line of a harness's file in, the [`TranscriptRecord`]s it
//!   carries out. Written harness by harness (`claude`, `copilot`, `pi`) and
//!   dispatched through [`crate::domain::probes::HarnessProbes::transcript_records`]
//!   (ADR-0051): the model's **thinking is dropped here**, at the reading, so no
//!   layer above can ever show it.
//! - **index** — [`TranscriptIndex`] folds the records of a file it is fed
//!   **incrementally** (only the bytes appended since its last feed), keeps the
//!   visible messages and, for a tool call, its name, a one-line summary and the
//!   **byte spans** of the lines that hold its input and its output — never the
//!   input or the output themselves.
//! - **views** — a page of entries ([`TranscriptPage`]) and one tool call's
//!   detail ([`ToolCallDetail`]), re-read from its two spans only.
//!
//! The adapter that owns the file handle and the per-file memo is
//! `crate::adapters::transcript_reader`.

mod claude;
mod copilot;
mod diff;
mod index;
mod pi;

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

pub(crate) use claude::records as claude_records;
pub(crate) use copilot::records as copilot_records;
pub use diff::{diff_of, diff_stat, DiffLine, DiffOp, DiffStat};
pub use index::{tool_detail, ToolSpans, TranscriptIndex};
pub(crate) use pi::records as pi_records;

/// Who speaks in a visible message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TranscriptSpeaker")]
pub enum Speaker {
    /// A user turn: the launch prompt, a steering message, a runtime message.
    Prompt,
    /// Text the model wrote for the reader (never its thinking).
    Agent,
}

/// What one line of a harness's session file carries, normalized.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptRecord {
    /// A visible message. `key` dedups a line the harness wrote twice (a resume
    /// replay): the same key is indexed once.
    Message {
        speaker: Speaker,
        text: String,
        at: Option<String>,
        key: Option<String>,
    },
    /// A tool call, with the input the model gave it.
    ToolCall {
        call_id: String,
        name: String,
        input: Value,
        at: Option<String>,
    },
    /// The outcome of the tool call `call_id`.
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
}

/// Where a line sits in its file: what a tool call's detail re-reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub offset: u64,
    pub len: u64,
}

/// A tool call's state as the index knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TranscriptToolStatus")]
pub enum ToolStatus {
    /// No result yet: the tool is running (or the session died in it).
    Pending,
    Ok,
    Error,
}

/// One entry of a page: a visible message, or a folded tool call.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, rename = "TranscriptEntry")]
pub enum TranscriptEntry {
    Message {
        /// Position in the transcript, stable while the file only grows.
        index: u32,
        speaker: Speaker,
        text: String,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
    Tool {
        index: u32,
        name: String,
        /// One line drawn from the input (a path, a command, a pattern…).
        summary: String,
        status: ToolStatus,
        /// The size of the output, once there is one.
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        output_bytes: Option<u64>,
        /// The lines a call that replaces text adds and removes.
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<DiffStat>,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
}

/// A page of a node iteration's transcript.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "TranscriptPage")]
pub struct TranscriptPage {
    /// `false` when PDO reads no transcript for this harness (or this node runs
    /// no agent): the tab says so, it is not an error.
    pub available: bool,
    /// The harness the iteration was launched with, when known.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The entries from `after` on, at most the page's limit.
    pub entries: Vec<TranscriptEntry>,
    /// Every entry of the transcript (messages and tool calls).
    pub total: u32,
    /// Visible messages, out of `total`.
    pub messages: u32,
    /// Folded tool calls, out of `total`.
    pub tool_calls: u32,
    /// The `after` of the next page, or `None` when this page reaches the end.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
}

impl TranscriptPage {
    /// The page of a harness PDO reads no transcript for.
    pub fn unavailable(harness: Option<String>) -> Self {
        TranscriptPage {
            available: false,
            harness,
            entries: Vec::new(),
            total: 0,
            messages: 0,
            tool_calls: 0,
            next: None,
        }
    }

    /// The page of a readable harness whose file does not exist yet (a node
    /// not started, a session that has written nothing).
    pub fn empty(harness: Option<String>) -> Self {
        TranscriptPage {
            available: true,
            ..Self::unavailable(harness)
        }
    }
}

/// A tool call opened: its input and its output, read on demand.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "ToolCallDetail")]
pub struct ToolCallDetail {
    pub name: String,
    /// The input, pretty-printed JSON (a bare string stays as is).
    pub input: String,
    /// The line diff of a call that replaces text, shown instead of its input;
    /// empty for any other call.
    pub diff: Vec<DiffLine>,
    /// The output, or `None` while the tool runs.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub is_error: bool,
    /// The output was cut at [`DETAIL_OUTPUT_CAP`] bytes.
    pub truncated: bool,
}

/// The most of a tool output a detail carries; the rest stays in the file.
pub const DETAIL_OUTPUT_CAP: usize = 64 * 1024;

/// The longest a tool call's one-line summary gets.
const SUMMARY_CAP: usize = 120;

/// The input keys a summary prefers, in order: what a reader recognises a call
/// by (the file, the command, the pattern…).
const SUMMARY_KEYS: [&str; 12] = [
    "pattern",
    "query",
    "command",
    "cmd",
    "file_path",
    "path",
    "notebook_path",
    "url",
    "skill",
    "description",
    "prompt",
    "subagent_type",
];

/// One line drawn from a tool call's input.
pub fn summarize(input: &Value) -> String {
    let picked = match input {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => SUMMARY_KEYS
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str).map(str::to_string))
            .or_else(|| map.values().find_map(|v| v.as_str().map(str::to_string))),
        _ => None,
    };
    let line = picked
        .as_deref()
        .and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("")
        .to_string();
    if line.chars().count() > SUMMARY_CAP {
        let cut: String = line.chars().take(SUMMARY_CAP - 1).collect();
        format!("{cut}…")
    } else {
        line
    }
}

/// The text of a `content` that is a string or an array of `text` blocks
/// (claude, pi, the tool results of both); other blocks (images, thinking,
/// tool calls) carry no text here.
pub(crate) fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A top-level string field of a line.
pub(crate) fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests;
