use super::*;
use crate::domain::probes;
use crate::domain::registry::{CLAUDE, COPILOT, OPENCODE, PI};

const CLAUDE_FIXTURE: &str = include_str!("../../../tests/fixtures/transcript/claude.jsonl");
const COPILOT_FIXTURE: &str = include_str!("../../../tests/fixtures/transcript/copilot.jsonl");
const PI_FIXTURE: &str = include_str!("../../../tests/fixtures/transcript/pi.jsonl");

fn index(harness: &str, text: &str) -> TranscriptIndex {
    let mut index = TranscriptIndex::new();
    index.feed(text.as_bytes(), |line| {
        probes::transcript_records(harness, line)
    });
    index
}

/// A compact view of a page: `P:text`, `A:text`, `T:name(summary)=status`.
fn shape(page: &TranscriptPage) -> Vec<String> {
    page.entries
        .iter()
        .map(|e| match e {
            TranscriptEntry::Message { speaker, text, .. } => match speaker {
                Speaker::Prompt => format!("P:{text}"),
                Speaker::Agent => format!("A:{text}"),
            },
            TranscriptEntry::Tool {
                name,
                summary,
                status,
                ..
            } => format!("T:{name}({summary})={status:?}"),
        })
        .collect()
}

fn whole(harness: &str, text: &str) -> TranscriptPage {
    index(harness, text).page(Some(harness.to_string()), 0, 1000)
}

#[test]
fn claude_fixture_reads_visible_turns_and_folds_tools() {
    let page = whole(CLAUDE, CLAUDE_FIXTURE);
    assert_eq!(
        shape(&page),
        vec![
            "P:Implement the search filter described in spec.md.",
            "T:Read(/repo/spec.md)=Ok",
            "A:The spec asks for a debounce on the input.",
            "T:Bash(cargo test)=Error",
            "A:Should the debounce also apply to the filter chips?",
            "P:Only to typing.",
            "T:Edit(/repo/src/search.ts)=Pending",
        ]
    );
    assert_eq!((page.total, page.messages, page.tool_calls), (7, 4, 3));
    assert_eq!(page.next, None);
}

#[test]
fn copilot_fixture_reads_visible_turns_and_folds_tools() {
    let page = whole(COPILOT, COPILOT_FIXTURE);
    assert_eq!(
        shape(&page),
        vec![
            "P:Implement the search filter described in spec.md.",
            "T:view(/repo/spec.md)=Ok",
            "T:grep(useSearch)=Error",
            "A:The spec asks for a debounce on the input.",
            "P:Only to typing.",
        ]
    );
}

#[test]
fn pi_fixture_reads_visible_turns_and_folds_tools() {
    let page = whole(PI, PI_FIXTURE);
    assert_eq!(
        shape(&page),
        vec![
            "P:Implement the search filter described in spec.md.",
            "T:read(/repo/spec.md)=Ok",
            "A:The spec asks for a debounce on the input.",
            "T:bash(npm test)=Error",
            "P:Only to typing.",
        ]
    );
}

#[test]
fn thinking_is_dropped_at_the_reading_for_every_harness() {
    for (harness, fixture) in [
        (CLAUDE, CLAUDE_FIXTURE),
        (COPILOT, COPILOT_FIXTURE),
        (PI, PI_FIXTURE),
    ] {
        assert!(
            fixture.contains("SECRET-THOUGHT"),
            "{harness}: fixture carries thinking"
        );
        let json = serde_json::to_string(&whole(harness, fixture)).unwrap();
        assert!(
            !json.contains("SECRET-THOUGHT"),
            "{harness}: thinking leaked"
        );
        for line in fixture.lines() {
            for record in probes::transcript_records(harness, line) {
                assert!(
                    !format!("{record:?}").contains("SECRET-THOUGHT"),
                    "{harness}"
                );
            }
        }
    }
}

#[test]
fn claude_drops_sidechain_meta_and_replayed_lines() {
    let json = serde_json::to_string(&whole(CLAUDE, CLAUDE_FIXTURE)).unwrap();
    assert!(!json.contains("SIDECHAIN"));
    assert!(!json.contains("local-command-caveat"));
    let replies = whole(CLAUDE, CLAUDE_FIXTURE)
        .entries
        .iter()
        .filter(|e| matches!(e, TranscriptEntry::Message { text, .. } if text.contains("asks for a debounce")))
        .count();
    assert_eq!(replies, 1, "a replayed line (same uuid) is indexed once");
}

#[test]
fn a_harness_without_a_reader_reads_nothing() {
    assert!(probes::can_read_transcript(CLAUDE));
    assert!(probes::can_read_transcript(COPILOT));
    assert!(probes::can_read_transcript(PI));
    assert!(!probes::can_read_transcript(OPENCODE));
    assert!(!probes::can_read_transcript("my-custom-harness"));
    for line in CLAUDE_FIXTURE.lines() {
        assert!(probes::transcript_records(OPENCODE, line).is_empty());
    }
}

#[test]
fn pages_cut_the_index_and_point_to_the_next() {
    let index = index(CLAUDE, CLAUDE_FIXTURE);
    let first = index.page(None, 0, 3);
    assert_eq!(first.entries.len(), 3);
    assert_eq!(first.next, Some(3));
    assert_eq!(first.total, 7);
    let second = index.page(None, 3, 3);
    assert_eq!(shape(&second)[0], "T:Bash(cargo test)=Error");
    assert_eq!(second.next, Some(6));
    let last = index.page(None, 6, 3);
    assert_eq!(last.entries.len(), 1);
    assert_eq!(last.next, None);
    let past = index.page(None, 40, 3);
    assert!(past.entries.is_empty());
    assert_eq!(past.total, 7);
    let indexes: Vec<u32> = second
        .entries
        .iter()
        .map(|e| match e {
            TranscriptEntry::Message { index, .. } | TranscriptEntry::Tool { index, .. } => *index,
        })
        .collect();
    assert_eq!(indexes, vec![3, 4, 5]);
}

#[test]
fn a_torn_last_line_waits_for_the_next_feed() {
    let full = CLAUDE_FIXTURE.as_bytes();
    let cut = CLAUDE_FIXTURE.find("Only to typing").unwrap();
    let mut index = TranscriptIndex::new();
    let records = |line: &str| probes::transcript_records(CLAUDE, line);
    let consumed = index.feed(&full[..cut], records);
    assert!(consumed < cut as u64, "the partial line is not consumed");
    assert_eq!(index.parsed_to(), consumed);
    let before = index.len();
    index.feed(&full[consumed as usize..], records);
    assert_eq!(index.parsed_to(), full.len() as u64);
    assert_eq!(index.len(), before + 2);
    assert_eq!(
        shape(&index.page(None, 0, 100)),
        shape(&whole(CLAUDE, CLAUDE_FIXTURE)),
        "feeding in two chunks indexes exactly what one feed does"
    );
}

#[test]
fn a_result_written_after_its_call_settles_the_call() {
    let lines: Vec<&str> = PI_FIXTURE.lines().collect();
    let records = |line: &str| probes::transcript_records(PI, line);
    let mut index = TranscriptIndex::new();
    let head = lines[..4].join("\n") + "\n";
    index.feed(head.as_bytes(), records);
    assert_eq!(
        shape(&index.page(None, 0, 10)).last().unwrap(),
        "T:read(/repo/spec.md)=Pending"
    );
    let rest = lines[4..].join("\n") + "\n";
    index.feed(rest.as_bytes(), records);
    assert_eq!(
        shape(&index.page(None, 0, 10))[1],
        "T:read(/repo/spec.md)=Ok"
    );
}

fn detail_of(harness: &str, fixture: &str, entry: usize) -> ToolCallDetail {
    let index = index(harness, fixture);
    let spans = index.tool_spans(entry).expect("a tool entry");
    let slice = |span: Span| &fixture[span.offset as usize..(span.offset + span.len) as usize];
    tool_detail(
        &spans.call_id,
        slice(spans.call),
        spans.result.map(slice),
        |line| probes::transcript_records(harness, line),
    )
    .unwrap()
}

#[test]
fn a_tool_detail_is_read_back_from_its_two_spans() {
    let read = detail_of(CLAUDE, CLAUDE_FIXTURE, 1);
    assert_eq!(read.name, "Read");
    assert!(read.input.contains("\"file_path\": \"/repo/spec.md\""));
    assert_eq!(read.output.as_deref(), Some("# Spec\nAdd a debounce."));
    assert!(!read.is_error);

    let bash = detail_of(CLAUDE, CLAUDE_FIXTURE, 3);
    assert!(bash.is_error);
    assert_eq!(bash.output.as_deref(), Some("exit 101: 2 tests failed"));

    let pending = detail_of(CLAUDE, CLAUDE_FIXTURE, 6);
    assert_eq!(pending.output, None);

    // Two calls on one copilot line: each detail picks its own.
    let grep = detail_of(COPILOT, COPILOT_FIXTURE, 2);
    assert_eq!(grep.name, "grep");
    assert!(grep.input.contains("useSearch"));
    assert_eq!(grep.output.as_deref(), Some("Path does not exist"));
    assert!(grep.is_error);

    let bash = detail_of(PI, PI_FIXTURE, 3);
    assert_eq!(bash.output.as_deref(), Some("1 failing"));
    assert!(bash.is_error);

    let index = index(CLAUDE, CLAUDE_FIXTURE);
    assert!(
        index.tool_spans(0).is_none(),
        "a message has no tool detail"
    );
    assert!(index.tool_spans(99).is_none());
}

#[test]
fn a_long_output_is_cut_on_a_char_boundary() {
    let long = "é".repeat(DETAIL_OUTPUT_CAP);
    let call = r#"{"type":"message","id":"a","message":{"role":"assistant","content":[{"type":"toolCall","id":"t","name":"read","arguments":{}}]}}"#;
    let result = serde_json::json!({
        "type": "message", "id": "b",
        "message": {"role": "toolResult", "toolCallId": "t", "content": [{"type": "text", "text": long}]}
    })
    .to_string();
    let detail = tool_detail("t", call, Some(&result), pi_records).unwrap();
    assert!(detail.truncated);
    assert!(detail.output.unwrap().len() <= DETAIL_OUTPUT_CAP);
}

#[test]
fn summaries_prefer_what_names_the_call_and_stay_one_line() {
    use serde_json::json;
    assert_eq!(
        summarize(&json!({"file_path": "/a/b.rs", "old_string": "x"})),
        "/a/b.rs"
    );
    assert_eq!(
        summarize(&json!({"description": "d", "command": "ls\n-la"})),
        "ls"
    );
    assert_eq!(
        summarize(&json!({"pattern": "useSearch", "path": "src"})),
        "useSearch"
    );
    assert_eq!(summarize(&json!({"other": "first string"})), "first string");
    assert_eq!(summarize(&json!("raw")), "raw");
    assert_eq!(summarize(&json!(null)), "");
    let long = summarize(&json!({"command": "x".repeat(500)}));
    assert_eq!(long.chars().count(), 120);
    assert!(long.ends_with('…'));
}

#[test]
fn an_edit_counts_its_lines_folded_and_shows_its_diff_unfolded() {
    let page = whole(CLAUDE, CLAUDE_FIXTURE);
    let diffs: Vec<_> = page
        .entries
        .iter()
        .filter_map(|e| match e {
            TranscriptEntry::Tool { name, diff, .. } => Some((name.as_str(), *diff)),
            TranscriptEntry::Message { .. } => None,
        })
        .collect();
    let one_each = DiffStat {
        added: 1,
        removed: 1,
    };
    assert_eq!(
        diffs,
        [("Read", None), ("Bash", None), ("Edit", Some(one_each))]
    );

    let edit = detail_of(CLAUDE, CLAUDE_FIXTURE, 6);
    let lines: Vec<_> = edit.diff.iter().map(|l| (l.op, l.text.as_str())).collect();
    assert_eq!(lines, [(DiffOp::Remove, "a"), (DiffOp::Add, "b")]);
    assert!(detail_of(CLAUDE, CLAUDE_FIXTURE, 1).diff.is_empty());
}
