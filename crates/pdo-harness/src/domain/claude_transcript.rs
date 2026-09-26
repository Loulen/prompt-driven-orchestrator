//! Reading Claude Code's own artefacts: the pane's usage-limit menu, the
//! transcript's turn state, and the project directory it names after a working
//! directory (#290, #373, #469). Pure: text or a path in, a fact out.

use std::path::Path;

/// On-screen anchors for Claude Code's usage-limit interactive menu (#290).
///
/// The menu wording is NOT officially documented and DRIFTS across CC versions
/// (corroborated by anthropics/claude-code#28484 + a direct capture 2026-06-30).
/// These are the substrings observed most stable; match is case-insensitive after
/// ANSI-stripping + whitespace-collapsing. Detection is best-effort /
/// observability-only (#290 Slice 1): a miss is the status quo (no regression), a
/// false positive is one harmless informational event. UPDATE THIS LIST when CC
/// wording changes.
const USAGE_LIMIT_ANCHORS: &[&str] = &[
    "stop and wait for limit to reset",
    "stop and wait for the limit to reset",
];

/// Strip ANSI/CSI escape sequences from a tmux pane capture (which is taken with
/// `-e`, so it contains escapes). Char-safe: preserves multi-byte UTF-8 (e.g. the
/// menu's `❯`). Best-effort — handles CSI (`ESC [ … final @-~`) and drops a lone
/// escape's next char; good enough for pane text (mostly SGR colour codes).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if matches!(nc, '\u{40}'..='\u{7e}') {
                        break; // CSI final byte
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True if the captured pane shows Claude Code's usage-limit interactive menu.
/// `pane` is raw tmux capture (may contain ANSI). Observability-only (#290): the
/// caller flags the node but never changes its fate.
///
/// `pub(crate)` since #613/ADR-0051: this is **`claude`'s** usage-limit
/// implementation ([`crate::domain::probes::HarnessProbes::detect_usage_limit`]),
/// not a generic matcher — a consumer dispatches through
/// [`crate::domain::probes::usage_limit_shown`], never calling this directly.
pub(crate) fn detect_usage_limit(pane: &str) -> bool {
    // Whitespace is collapsed so a line-wrap / padding can't split an anchor.
    let stripped = strip_ansi(pane).to_ascii_lowercase();
    let norm = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    USAGE_LIMIT_ANCHORS.iter().any(|a| norm.contains(a))
}

/// Where an agent's transcript says it is, right now (#469 §2).
///
/// Exactly one variant is actionable ([`Self::TurnEnded`]); the other three all
/// mean "leave it alone", for three different reasons worth keeping distinct in
/// logs and tests.
///
/// A *substantial* record is one carrying a `message` object whose `role` is
/// `assistant` or `user`. That definition is load-bearing: a naive "look at the
/// last line" reads one of Claude Code's trailing metadata records
/// (`last-prompt`, `ai-title`, `mode`, `permission-mode` — none of which even
/// carry a `timestamp`) and concludes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    /// A `tool_use` block has no matching `tool_result`: the agent is *inside* a
    /// tool call and alive, however long the silence has lasted. This is the
    /// state a `docker build` or a `cargo test --workspace` sits in, and the one
    /// the old mtime threshold could not tell from death.
    InToolCall,
    /// The last substantial record is a `user` message (a prompt, or a
    /// `tool_result`): the assistant still owes a reply. **This is the
    /// API-retries-exhausted shape (#251)** — it must never be completed.
    AwaitingAssistant,
    /// The last substantial record is an `assistant` message and no `tool_use` is
    /// pending: the turn is over. The only actionable state.
    TurnEnded,
    /// No transcript, nothing parseable, or a single record overrunning the read
    /// window (a large `tool_result`). Behaves as "at work": with the signal
    /// absent nothing is touched. Fail-safe by construction.
    Unknown,
}

/// Role of a substantial transcript record. Private: only the last one matters
/// to [`parse_turn_state`], and only to distinguish two of its four answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordRole {
    Assistant,
    User,
}

/// Classify the tail of a Claude Code `.jsonl` transcript into a [`TurnState`]
/// (#469 §2). Pure — the caller does the read ([`read_transcript_tail`]).
///
/// One forward pass over the lines:
/// 1. every `tool_use` block seen in an `assistant` message opens an id;
/// 2. every `tool_result` block seen in a `user` message closes its
///    `tool_use_id`;
/// 3. an id still open at the end ⇒ [`TurnState::InToolCall`], **checked first**
///    — the record that opened it is itself an `assistant` message, so testing
///    the last role first would misread a pending tool call as a finished turn;
/// 4. otherwise the last substantial role decides.
///
/// Unparseable lines are skipped, which is what makes a byte-clipped tail safe:
/// the leading partial record is simply not there. A `tool_result` whose
/// `tool_use` fell outside the window closes nothing (harmless); a `tool_use`
/// whose `tool_result` fell outside stays open (conservative).
///
/// The JSONL layout is not a documented contract — same caution as the #290 pane
/// anchors — though `tool_use` / `tool_result` blocks and their `id`s are its
/// most stable part, far ahead of a menu's wording.
///
/// `pub(crate)` since #613/ADR-0051: this is **`claude`'s** end-of-turn parser
/// ([`crate::domain::probes::HarnessProbes::classify_turn_ended`]) — a consumer
/// dispatches through [`crate::domain::probes::turn_ended`], never reading another
/// harness's store with it.
pub(crate) fn parse_turn_state(tail: &str) -> TurnState {
    let mut open_tool_uses: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_role: Option<RecordRole> = None;

    for line in tail.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = record.get("message") else {
            continue;
        };
        let role = match message.get("role").and_then(|r| r.as_str()) {
            Some("assistant") => RecordRole::Assistant,
            Some("user") => RecordRole::User,
            _ => continue,
        };
        last_role = Some(role);

        // `content` is an array of blocks in the modern format and a bare string
        // in the older one; only the array can carry tool blocks.
        if let Some(blocks) = message.get("content").and_then(|c| c.as_array()) {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                            open_tool_uses.insert(id.to_string());
                        }
                    }
                    Some("tool_result") => {
                        if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str()) {
                            open_tool_uses.remove(id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !open_tool_uses.is_empty() {
        return TurnState::InToolCall;
    }
    match last_role {
        Some(RecordRole::Assistant) => TurnState::TurnEnded,
        Some(RecordRole::User) => TurnState::AwaitingAssistant,
        None => TurnState::Unknown,
    }
}

/// Encode a working directory path exactly as Claude Code names its
/// `~/.claude/projects/` directory: every non-`[A-Za-z0-9]` char maps to `-`
/// (case preserved, runs NOT collapsed). So a leading `/` becomes a leading `-`
/// and `.pdo`/`.claude` become `--pdo`/`--claude`.
///
/// Example: `/home/user/project` → `-home-user-project`; a PDO node dir like
/// `/home/u/.pdo/runs/X/worktree` → `-home-u--pdo-runs-X-worktree`.
///
/// This is the single source of truth for the CC project-dir encoding;
/// [`cc_project_dirname`] delegates here.
///
/// Don't "clean up" the encoding (strip the leading `/`, keep `.` intact): that
/// resolved `None` for *every* PDO node, silently killing the transcript probe
/// (#373).
pub fn encode_working_dir(dir: &Path) -> String {
    dir.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The Claude Code project dirname for a working directory — the encoded cwd
/// ([`encode_working_dir`]), under which `claude` writes a session's transcript.
pub fn cc_project_dirname(path: &Path) -> String {
    encode_working_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- detect_usage_limit / strip_ansi (#290) ---

    #[test]
    fn detects_usage_limit_menu_with_ansi() {
        // A realistic capture: the selected line carries SGR colour codes and the
        // `❯` cursor glyph, wrapped by the surrounding menu text.
        let pane = "What do you want to do?\n\x1b[2m❯\x1b[0m 1. Stop and \
                    wait for limit to reset\n  2. Switch to usage credits\n";
        assert!(detect_usage_limit(pane));
    }

    #[test]
    fn detects_the_the_variant() {
        assert!(detect_usage_limit(
            "…please wait for the limit to reset. Stop and wait for the limit to reset\n"
        ));
    }

    #[test]
    fn case_insensitive() {
        assert!(detect_usage_limit("STOP AND WAIT FOR LIMIT TO RESET"));
    }

    #[test]
    fn wrapped_anchor_still_matches() {
        // The anchor split by a newline + padding (as a narrow pane would wrap it)
        // must still match — proves the whitespace-collapse normalisation.
        assert!(detect_usage_limit(
            "stop and wait\n   for   limit\nto reset"
        ));
    }

    #[test]
    fn normal_running_pane_is_not_flagged() {
        let pane = "\x1b[2m✻\x1b[0m Thinking… (esc to interrupt)\n\
                    ● Running: cargo test -p pdo-daemon\n";
        assert!(!detect_usage_limit(pane));
    }

    #[test]
    fn empty_pane_is_not_flagged() {
        assert!(!detect_usage_limit(""));
    }

    #[test]
    fn strip_ansi_preserves_unicode() {
        assert_eq!(strip_ansi("\x1b[1m❯\x1b[0m x"), "❯ x");
    }

    // --- encode_working_dir (#373: matches Claude Code's real scheme) ---

    #[test]
    fn encode_basic_path_keeps_leading_dash() {
        // Every non-alphanumeric maps to `-`, so a leading `/` becomes `-`.
        assert_eq!(
            encode_working_dir(Path::new("/home/user/project")),
            "-home-user-project"
        );
    }

    #[test]
    fn encode_root() {
        assert_eq!(encode_working_dir(Path::new("/")), "-");
    }

    #[test]
    fn encode_deeply_nested() {
        assert_eq!(encode_working_dir(Path::new("/a/b/c/d/e")), "-a-b-c-d-e");
    }

    #[test]
    fn encode_maps_dot_to_dash() {
        // #373 root cause: a real PDO node dir carries `.pdo` (→ `--pdo`) and a
        // leading `-`. Before the fix this produced `home-...-.pdo-...` and
        // resolved nothing under ~/.claude/projects.
        assert_eq!(
            encode_working_dir(Path::new("/home/u/.pdo/runs/X/worktree")),
            "-home-u--pdo-runs-X-worktree"
        );
    }

    #[test]
    fn encode_matches_cc_project_dirname() {
        // The two encoders are unified on one implementation (#373): they must
        // never drift again.
        for dir in [
            "/home/user/code/pdo/.pdo/runs/2026-abc/nodes/n1/iter-1",
            "/home/u/.claude",
            "/tmp/x.y.z",
        ] {
            assert_eq!(
                encode_working_dir(Path::new(dir)),
                cc_project_dirname(Path::new(dir)),
                "encode_working_dir and cc_project_dirname must agree for {dir}"
            );
        }
    }

    // --- parse_turn_state (#469 §2). The fixtures are cut from the REAL
    // transcript of the node this issue was opened about (`XBG5Cxkn`, 679
    // records); free text is clipped, every field the parser reads is verbatim.

    /// The real tail: … assistant `tool_use` → user `tool_result` → assistant
    /// `text` → two `system` records → four untimestamped metadata records.
    const FIXTURE_TURN_ENDED: &str =
        include_str!("../../tests/fixtures/turn_state/turn_ended.jsonl");
    /// The same transcript cut on the `docker build -q` whose `tool_result`
    /// landed **214 s** later — one of five over-threshold gaps in this healthy
    /// node, and the measured cause of the false `node_stale`.
    const FIXTURE_IN_TOOL_CALL: &str =
        include_str!("../../tests/fixtures/turn_state/in_tool_call.jsonl");
    /// Cut on a `user`/`tool_result`: the assistant still owes a reply.
    const FIXTURE_AWAITING_ASSISTANT: &str =
        include_str!("../../tests/fixtures/turn_state/awaiting_assistant.jsonl");
    /// Only Claude Code's untimestamped metadata records (`last-prompt`,
    /// `ai-title`, `mode`, `permission-mode`).
    const FIXTURE_METADATA_ONLY: &str =
        include_str!("../../tests/fixtures/turn_state/metadata_only.jsonl");

    #[test]
    fn real_transcript_tail_is_turn_ended() {
        // AC4. The node this issue is about HAD finished: its last substantial
        // record is an assistant `text` message and no tool call is pending.
        assert_eq!(parse_turn_state(FIXTURE_TURN_ENDED), TurnState::TurnEnded);
    }

    #[test]
    fn pending_docker_build_is_in_tool_call() {
        // AC5, the fixture that forbids "two writers on one worktree": mid a
        // 214 s `docker build` the agent is *inside* a tool call, so neither a
        // duration nor already-valid outputs may complete it.
        assert_eq!(
            parse_turn_state(FIXTURE_IN_TOOL_CALL),
            TurnState::InToolCall
        );
    }

    #[test]
    fn trailing_tool_result_is_awaiting_assistant() {
        // AC6 — the API-retries-exhausted shape (#251).
        assert_eq!(
            parse_turn_state(FIXTURE_AWAITING_ASSISTANT),
            TurnState::AwaitingAssistant
        );
    }

    #[test]
    fn metadata_only_tail_is_unknown_not_turn_ended() {
        // AC7. These records carry no `message`, so a naive "read the last line"
        // would see `permission-mode` and conclude nothing — hence the
        // *substantial record* definition. `Unknown` behaves as "at work".
        assert_eq!(parse_turn_state(FIXTURE_METADATA_ONLY), TurnState::Unknown);
    }

    #[test]
    fn empty_tail_is_unknown() {
        assert_eq!(parse_turn_state(""), TurnState::Unknown);
        assert_eq!(parse_turn_state("\n\n  \n"), TurnState::Unknown);
    }

    #[test]
    fn a_clipped_leading_record_is_skipped_not_guessed() {
        // The read window can land mid-record. The leading partial line must be
        // dropped, and the verdict stays that of the records which do survive.
        assert_eq!(
            parse_turn_state(&FIXTURE_TURN_ENDED[40..]),
            TurnState::TurnEnded
        );
    }

    #[test]
    fn a_single_unparseable_record_is_unknown() {
        // One oversized `tool_result` overrunning the whole window: nothing
        // substantial survives → `Unknown` → left alone. Fail-safe.
        assert_eq!(
            parse_turn_state(r#"{"type":"user","message":{"role":"user","conte"#),
            TurnState::Unknown
        );
    }

    #[test]
    fn pending_tool_use_beats_a_later_assistant_message() {
        // Ordering guard: the record that OPENS a tool call is itself an
        // `assistant` message, so testing the last role before the pending-call
        // set would read a live tool call as a finished turn.
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 300"}}]}}"#;
        assert_eq!(parse_turn_state(tail), TurnState::InToolCall);
    }

    #[test]
    fn a_matched_tool_call_does_not_hold_the_turn_open() {
        let tail = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn an_orphan_tool_result_closes_nothing_and_is_harmless() {
        // Its `tool_use` fell outside the read window. Removing an id that was
        // never inserted is a no-op, and the last role still decides.
        let tail = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"gone","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn a_string_content_assistant_message_still_ends_the_turn() {
        // The older format carries `content` as a bare string: no tool blocks to
        // read, but the role still counts.
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":"all done"}}"#;
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }

    #[test]
    fn system_records_are_not_substantial() {
        // A `system` record has no `message`, so it never decides; a `thinking`
        // block is an assistant message and legitimately does.
        let tail = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            "\n",
            r#"{"type":"system","timestamp":"2026-07-29T09:10:43.884Z","hasOutput":false}"#,
            "\n"
        );
        assert_eq!(parse_turn_state(tail), TurnState::TurnEnded);
    }
}
