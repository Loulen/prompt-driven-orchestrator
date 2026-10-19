//! The incremental index of one transcript file, and a tool call's detail
//! read back from the two lines the index points at.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::{
    diff_of, diff_stat, summarize, DiffStat, Span, Speaker, ToolCallDetail, ToolStatus,
    TranscriptEntry, TranscriptPage, TranscriptRecord, DETAIL_OUTPUT_CAP,
};

#[derive(Debug, Clone, PartialEq)]
enum Indexed {
    Message {
        speaker: Speaker,
        text: String,
    },
    Tool {
        call_id: String,
        name: String,
        summary: String,
        diff: Option<DiffStat>,
        call: Span,
        result: Option<(Span, bool, u64)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedEntry {
    at: Option<String>,
    body: Indexed,
}

/// The incremental index of one transcript file.
///
/// Fed the bytes appended since its last feed ([`Self::feed`]); a partial last
/// line is left for the next feed, so a live session's torn tail is never
/// parsed. `records` is the harness's line reader.
#[derive(Debug, Clone, Default)]
pub struct TranscriptIndex {
    entries: Vec<IndexedEntry>,
    /// Where the next feed starts: the end of the last complete line.
    parsed_to: u64,
    seen_keys: HashSet<String>,
    calls: HashMap<String, usize>,
    messages: u32,
    tool_calls: u32,
}

impl TranscriptIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// The byte offset the next [`Self::feed`] must start at.
    pub fn parsed_to(&self) -> u64 {
        self.parsed_to
    }

    /// Fold `chunk` — the file's bytes from [`Self::parsed_to`] on — into the
    /// index, complete lines only. Returns how many bytes were consumed.
    pub fn feed(&mut self, chunk: &[u8], records: impl Fn(&str) -> Vec<TranscriptRecord>) -> u64 {
        let mut consumed = 0usize;
        while let Some(nl) = chunk[consumed..].iter().position(|b| *b == b'\n') {
            let line = &chunk[consumed..consumed + nl];
            let span = Span {
                offset: self.parsed_to + consumed as u64,
                len: nl as u64,
            };
            if let Ok(text) = std::str::from_utf8(line) {
                if !text.trim().is_empty() {
                    for record in records(text) {
                        self.fold(record, span);
                    }
                }
            }
            consumed += nl + 1;
        }
        self.parsed_to += consumed as u64;
        consumed as u64
    }

    fn fold(&mut self, record: TranscriptRecord, span: Span) {
        match record {
            TranscriptRecord::Message {
                speaker,
                text,
                at,
                key,
            } => {
                if text.trim().is_empty() {
                    return;
                }
                if let Some(key) = key {
                    if !self.seen_keys.insert(key) {
                        return;
                    }
                }
                self.messages += 1;
                self.entries.push(IndexedEntry {
                    at,
                    body: Indexed::Message { speaker, text },
                });
            }
            TranscriptRecord::ToolCall {
                call_id,
                name,
                input,
                at,
            } => {
                if self.calls.contains_key(&call_id) {
                    return;
                }
                self.calls.insert(call_id.clone(), self.entries.len());
                self.tool_calls += 1;
                self.entries.push(IndexedEntry {
                    at,
                    body: Indexed::Tool {
                        call_id,
                        summary: summarize(&input),
                        diff: diff_stat(&input),
                        name,
                        call: span,
                        result: None,
                    },
                });
            }
            TranscriptRecord::ToolResult {
                call_id,
                output,
                is_error,
            } => {
                let Some(&at) = self.calls.get(&call_id) else {
                    return;
                };
                if let Indexed::Tool { result, .. } = &mut self.entries[at].body {
                    if result.is_none() {
                        *result = Some((span, is_error, output.len() as u64));
                    }
                }
            }
        }
    }

    /// Every entry indexed so far.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The page of at most `limit` entries from `after` on.
    pub fn page(&self, harness: Option<String>, after: usize, limit: usize) -> TranscriptPage {
        let end = after.saturating_add(limit).min(self.entries.len());
        let start = after.min(end);
        let entries = self.entries[start..end]
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let index = (start + i) as u32;
                match &entry.body {
                    Indexed::Message { speaker, text } => TranscriptEntry::Message {
                        index,
                        speaker: *speaker,
                        text: text.clone(),
                        at: entry.at.clone(),
                    },
                    Indexed::Tool {
                        name,
                        summary,
                        diff,
                        result,
                        ..
                    } => TranscriptEntry::Tool {
                        index,
                        name: name.clone(),
                        summary: summary.clone(),
                        status: match result {
                            None => ToolStatus::Pending,
                            Some((_, true, _)) => ToolStatus::Error,
                            Some((_, false, _)) => ToolStatus::Ok,
                        },
                        output_bytes: result.map(|(_, _, bytes)| bytes),
                        diff: *diff,
                        at: entry.at.clone(),
                    },
                }
            })
            .collect();
        TranscriptPage {
            available: true,
            harness,
            entries,
            total: self.entries.len() as u32,
            messages: self.messages,
            tool_calls: self.tool_calls,
            next: (end < self.entries.len()).then_some(end as u32),
        }
    }

    /// What a tool call's detail re-reads: its id, its call line and, once it
    /// has one, its result line. `None` when `index` is not a tool call.
    pub fn tool_spans(&self, index: usize) -> Option<ToolSpans> {
        match &self.entries.get(index)?.body {
            Indexed::Tool {
                call_id,
                call,
                result,
                ..
            } => Some(ToolSpans {
                call_id: call_id.clone(),
                call: *call,
                result: result.map(|(span, _, _)| span),
            }),
            Indexed::Message { .. } => None,
        }
    }
}

/// Where a tool call's detail lives in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpans {
    pub call_id: String,
    pub call: Span,
    pub result: Option<Span>,
}

/// The detail of the tool call `call_id`, from its call line and its result
/// line (each re-parsed with the harness's own `records`; a line may carry
/// several calls).
pub fn tool_detail(
    call_id: &str,
    call_line: &str,
    result_line: Option<&str>,
    records: impl Fn(&str) -> Vec<TranscriptRecord>,
) -> Option<ToolCallDetail> {
    let (name, input) = records(call_line).into_iter().find_map(|r| match r {
        TranscriptRecord::ToolCall {
            call_id: id,
            name,
            input,
            ..
        } if id == call_id => Some((name, input)),
        _ => None,
    })?;
    let result = result_line.and_then(|line| {
        records(line).into_iter().find_map(|r| match r {
            TranscriptRecord::ToolResult {
                call_id: id,
                output,
                is_error,
            } if id == call_id => Some((output, is_error)),
            _ => None,
        })
    });
    let diff = diff_of(&input);
    let input = match input {
        Value::String(s) => s,
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    };
    let (output, is_error, truncated) = match result {
        None => (None, false, false),
        Some((mut output, is_error)) => {
            let truncated = output.len() > DETAIL_OUTPUT_CAP;
            if truncated {
                let mut cut = DETAIL_OUTPUT_CAP;
                while !output.is_char_boundary(cut) {
                    cut -= 1;
                }
                output.truncate(cut);
            }
            (Some(output), is_error, truncated)
        }
    };
    Some(ToolCallDetail {
        name,
        input,
        diff,
        output,
        is_error,
        truncated,
    })
}
