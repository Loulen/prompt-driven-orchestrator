//! Reading pi's per-session **JSONL** (#707, story #702; ADR-0052 amended, ADR-0043
//! applied to a third harness).
//!
//! `pi` is PDO's third first-party harness. Its session lives at
//! `<agent_dir>/sessions/<encoded-cwd>/<timestamp>_<session-id>.jsonl` — a directory
//! **named after the working directory** (like `claude`'s store) but a file **named
//! after the session identity PDO imposed** (`--session-id`, like `copilot`'s
//! journal). So two nodes sharing a worktree write two files in one directory, and
//! the resolution is a glob on `*_<id>.jsonl` inside the cwd's directory
//! ([`resolve_by_id`]), never a newest-mtime pick.
//!
//! This module is **pure** where it parses: session text in, facts out, no `$HOME`.
//! The one filesystem touch is [`resolve_by_id`] (a directory listing), because the
//! file name carries a creation timestamp PDO does not know. Its callers
//! (`crate::harness_probes` for the dispatch, `crate::run_cost` for the reported
//! cost, `crate::stats_performance` for the context peak) inject what they read.
//!
//! ## The facts, each measured on a `pi` 0.85.1 session
//!
//! - **Reported cost, already in dollars.** Every assistant message carries
//!   `usage.cost.total`, computed by pi from its embedded model catalogue. It is a
//!   **reported** cost (ADR-0052 §2) of conversion constant **1.0**
//!   ([`REPORTED_USD_CONSTANT`]): PDO sums it and never re-derives it from tokens.
//!   The constant being 1.0 is what lets a surface show it **without `~`**
//!   (CONTEXT.md § "Coût rapporté en dollars"). One hazard, decided at the
//!   grilling: a message with **tokens but no cost** (a catalogue absent from the
//!   home: pi warns and runs) is not free — it makes the node's total
//!   **unavailable** ([`ReportedCost::Unavailable`]), never `$0`.
//!
//! - **Context peak.** `usage.totalTokens` on each assistant message is that turn's
//!   full occupancy (input + output + both cache buckets, reasoning included in
//!   output). The peak is the max over deduplicated messages; the ceiling a reader
//!   holds it against is the model's context window as `pi --list-models` publishes
//!   it (#705, `Catalogue::model_contexts`).
//!
//! - **End of turn / hard error.** pi's `stopReason` is explicit: `stop` / `length`
//!   end the turn, `toolUse` opens tool calls that `toolResult` messages close,
//!   `error` (with `errorMessage`) is a **hard failure the process survives** — pi
//!   stays resident, so the exit code is no verdict (ADR-0052's copilot lesson) and
//!   the session text is. The sweep's fallback classifier ([`turn_state`]) reuses
//!   `claude`'s four-state vocabulary ([`crate::stale_detector::TurnState`]) so
//!   `assess_node` reads one verdict shape whatever the harness.
//!
//! - **Primary substrate: the turn-end extension.** The equivalent of `claude`'s
//!   injected `Stop` hook (ADR-0043): [`TURN_END_EXTENSION_TS`], written per node by
//!   the daemon and loaded through `-e` (the descriptor's `{settings}` hole), runs
//!   `pdo complete --auto` on `agent_settled` — the event pi documents as "no
//!   automatic retry, compaction retry or queued continuation remains". It never
//!   exits pi and never blocks the turn.

use crate::stale_detector::TurnState;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The conversion constant of pi's reported cost (ADR-0052 §2): pi's
/// `usage.cost.total` is **already in US dollars**, so the constant is **1.0** —
/// not an approximation, hence no `~` on the surfaces. Published here (rather than
/// inlined as a no-op) so the form of the cost stays legible: it is *reported*, of
/// constant 1.0, not *derived*.
pub(crate) const REPORTED_USD_CONSTANT: f64 = 1.0;

/// File-name suffix of the per-node turn-end extension the daemon writes beside the
/// prompt (`<node>-iter-<n>.turn-end.ts`), the twin of `claude`'s
/// `.settings.json`. TypeScript because pi loads extensions through jiti and its
/// documentation names `.ts` as the extension file format.
pub(crate) const TURN_END_EXTENSION_SUFFIX: &str = ".turn-end.ts";

/// The turn-end extension pi loads through `-e` (#707, ADR-0043 applied). Three
/// properties are load-bearing, mirroring `claude`'s `Stop` hook wrapper:
///
/// - **`agent_settled`, not `agent_end` / `turn_end`.** `agent_end` fires while pi
///   may still auto-retry or compact-and-retry; `turn_end` fires between tool
///   rounds. `agent_settled` is the one pi documents for "status integrations that
///   need to know Pi will not continue running automatically".
/// - **Never forces the exit, never blocks.** It runs `pdo complete --auto` and
///   swallows every outcome: a refused completion (exit 3 — an output still
///   missing, ADR-0035) leaves the session resident for the agent or the human, and
///   nothing here can loop or complete prematurely.
/// - **No env threaded in.** `pdo` is on the session PATH and the `PDO_*` exports of
///   the session shell are inherited by `pi.exec`'s child, exactly as the `Stop`
///   hook inherits them.
pub(crate) const TURN_END_EXTENSION_TS: &str = r#"// PDO turn-end extension for pi (#707, ADR-0043 applied to pi).
// Ephemeral: written per node by the daemon when `autocomplete_turn_end` is on and
// loaded through `-e`. On `agent_settled` (no retry, compaction retry or queued
// follow-up left) it signals `pdo complete --auto`. It never exits pi and never
// blocks: a refused completion (exit 3, an output still missing) leaves the session
// resident. `pdo` and the `PDO_*` env come from the session shell.
export default function (pi: any) {
  pi.on("agent_settled", async () => {
    try {
      await pi.exec("pdo", ["complete", "--auto"], { timeout: 60000 });
    } catch (_error) {
      // Best-effort: the sweep's session-tail fallback still covers the node.
    }
  });
}
"#;

/// The session directory pi names after a working directory, byte for byte as
/// `getDefaultSessionDirPath` does (0.85.1): **one** leading `/` (or `\`) dropped,
/// every remaining `/`, `\` and `:` turned into `-`, the whole wrapped in `--…--`.
/// So `/tmp/pdo/worktree` → `--tmp-pdo-worktree--`. Measured against real
/// directories under `~/.pi/agent/sessions/`.
pub(crate) fn session_dir_name(working_dir: &Path) -> String {
    let raw = working_dir.to_string_lossy();
    let stripped = raw
        .strip_prefix('/')
        .or_else(|| raw.strip_prefix('\\'))
        .unwrap_or(&raw);
    let encoded: String = stripped
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("--{encoded}--")
}

/// Resolve the session file of the identity PDO imposed: the one `*_<session_id>.jsonl`
/// in `<store_root>/<session_dir_name(working_dir)>/`. `None` when the directory or
/// the file does not exist yet (a node that has not written a first entry), or when
/// `session_id` is empty. Should pi ever leave two files for one id (it does not:
/// `--session-id` is created-or-resumed), the newest by mtime wins.
pub(crate) fn resolve_by_id(
    store_root: &Path,
    working_dir: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let dir = store_root.join(session_dir_name(working_dir));
    let suffix = format!("_{session_id}.jsonl");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(&suffix) {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

/// One assistant message's billing facts: its dedup key (the entry `id`), its
/// per-turn occupancy and its reported cost (`None` when the message carries no
/// `usage.cost` at all).
struct AssistantUsage {
    entry_id: Option<String>,
    total_tokens: u64,
    cost_total: Option<f64>,
}

/// Parse one session line into an [`AssistantUsage`], or `None` to skip it (a
/// non-message entry, a user / toolResult message, torn JSON). Tolerant: a session
/// file is external input, never trusted to be well-formed.
fn parse_assistant_usage(raw: &str) -> Option<AssistantUsage> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("message") {
        return None;
    }
    let msg = v.get("message")?;
    if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return None;
    }
    let usage = msg.get("usage")?;
    let field = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    // `totalTokens` is the published per-turn occupancy; the four-bucket sum is the
    // fallback for a writer that omits it.
    let total_tokens = usage
        .get("totalTokens")
        .and_then(|x| x.as_u64())
        .unwrap_or_else(|| {
            field("input") + field("output") + field("cacheRead") + field("cacheWrite")
        });
    let cost_total = usage
        .get("cost")
        .and_then(|c| c.get("total"))
        .and_then(|t| t.as_f64());
    Some(AssistantUsage {
        entry_id: v.get("id").and_then(|x| x.as_str()).map(String::from),
        total_tokens,
        cost_total,
    })
}

/// The deduplicated assistant messages of `text`, in order. Dedup is by the entry
/// `id` (pi's per-entry identity): a session re-opened by identity replays nothing,
/// but a defensive dedup costs nothing and matches `claude`'s `message.id` discipline.
fn deduplicated_assistant_usages(text: &str) -> Vec<AssistantUsage> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter_map(parse_assistant_usage)
        .filter(|u| match &u.entry_id {
            Some(id) => seen.insert(id.clone()),
            None => true,
        })
        .collect()
}

/// The reported cost of one pi session, as [`crate::run_cost`] folds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ReportedCost {
    /// No assistant message with tokens yet — a session that has not finished a
    /// first exchange. "No reading", distinct from "a reading of zero".
    NoReading,
    /// At least one message carries **tokens without a cost** (`usage.cost.total`
    /// absent or `0` while `totalTokens > 0`): pi ran without its model catalogue,
    /// so it could not price the turn. The node's total is **unavailable** — "—"
    /// with this reason — never `$0` (ADR-0052 §2 amended, CONTEXT.md § "Coût
    /// rapporté en dollars").
    Unavailable { reason: String },
    /// The sum of every deduplicated message's `usage.cost.total`, in USD (constant
    /// 1.0).
    Usd(f64),
}

/// The reason a token-bearing message without a cost makes the total unavailable.
/// Named here so `run_cost` and the tests agree on the wording a user reads.
pub(crate) const CATALOGUE_ABSENT_REASON: &str =
    "catalogue absent: pi reported tokens without a cost";

/// The session's reported cost (see [`ReportedCost`]). Pure: a torn line is skipped,
/// never propagated. A message with zero tokens (an errored or aborted call that
/// never reached the model) is neither a reading nor a hazard, and is ignored.
pub(crate) fn reported_cost(text: &str) -> ReportedCost {
    let mut total = 0.0;
    let mut any = false;
    for u in deduplicated_assistant_usages(text) {
        if u.total_tokens == 0 {
            continue;
        }
        match u.cost_total {
            Some(c) if c > 0.0 => {
                total += c * REPORTED_USD_CONSTANT;
                any = true;
            }
            _ => {
                return ReportedCost::Unavailable {
                    reason: CATALOGUE_ABSENT_REASON.to_string(),
                };
            }
        }
    }
    if any {
        ReportedCost::Usd(total)
    } else {
        ReportedCost::NoReading
    }
}

/// The session's context peak, in tokens — the maximum `usage.totalTokens` across
/// every deduplicated assistant message. `None` rather than `Some(0)` when no
/// message carries tokens, so an absent reading is never read as "used no context".
pub(crate) fn session_peak(text: &str) -> Option<u64> {
    deduplicated_assistant_usages(text)
        .into_iter()
        .map(|u| u.total_tokens)
        .filter(|t| *t > 0)
        .max()
}

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
pub(crate) fn turn_state(tail: &str) -> TurnState {
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
pub(crate) fn turn_ended(tail: &str) -> bool {
    turn_state(tail) == TurnState::TurnEnded
}

/// Whether this session `tail` trails on a **hard error** — an assistant message
/// whose `stopReason` is `error`, with its `errorMessage` (best-effort). This is the
/// failure pi's exit code cannot report: the process stays resident after it
/// (`exit_code_is_verdict` is `false` for pi), so the text is the verdict. A later
/// successful assistant message clears it (pi auto-retries).
pub(crate) fn hard_error(tail: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures cut from a real `pi` 0.85.1 session PDO launched (cwd anonymised,
    // text bodies trimmed, usage figures verbatim). Beside the catalogue fixtures.
    const TURN_ENDED: &str = include_str!("../tests/fixtures/pi-0.85.1/turn_ended.jsonl");
    const IN_TOOL_CALL: &str = include_str!("../tests/fixtures/pi-0.85.1/in_tool_call.jsonl");
    const AWAITING: &str = include_str!("../tests/fixtures/pi-0.85.1/awaiting_assistant.jsonl");
    const HARD_ERROR: &str = include_str!("../tests/fixtures/pi-0.85.1/hard_error.jsonl");
    const TOKENS_WITHOUT_COST: &str =
        include_str!("../tests/fixtures/pi-0.85.1/tokens_without_cost.jsonl");

    /// The five assistant messages of the fixture, `usage.cost.total` summed by hand.
    const FIXTURE_USD: f64 = 0.004496 + 0.003191 + 0.003654 + 0.005052 + 0.004289;

    #[test]
    fn reported_cost_sums_every_assistant_messages_cost_total_in_dollars() {
        match reported_cost(TURN_ENDED) {
            ReportedCost::Usd(usd) => assert!((usd - FIXTURE_USD).abs() < 1e-9, "{usd}"),
            other => panic!("expected a dollar figure, got {other:?}"),
        }
        assert_eq!(
            REPORTED_USD_CONSTANT, 1.0,
            "already in dollars: no conversion"
        );
    }

    #[test]
    fn reported_cost_dedups_a_replayed_entry_by_its_id() {
        let line = TURN_ENDED
            .lines()
            .find(|l| l.contains("\"role\":\"assistant\""))
            .unwrap();
        let replayed = format!("{line}\n{line}\n");
        match reported_cost(&replayed) {
            ReportedCost::Usd(usd) => assert!((usd - 0.004496).abs() < 1e-9, "{usd}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reported_cost_is_no_reading_before_a_first_assistant_message() {
        let header_and_prompt: String = TURN_ENDED.lines().take(4).collect::<Vec<_>>().join("\n");
        assert_eq!(reported_cost(&header_and_prompt), ReportedCost::NoReading);
        assert_eq!(reported_cost(""), ReportedCost::NoReading);
    }

    #[test]
    fn tokens_without_a_cost_make_the_total_unavailable_never_zero() {
        // The grilling's hazard: pi ran without its catalogue, priced nothing, and
        // wrote `cost.total: 0` beside thousands of tokens. Not free — unavailable.
        match reported_cost(TOKENS_WITHOUT_COST) {
            ReportedCost::Unavailable { reason } => {
                assert!(reason.contains("catalogue absent"), "{reason}")
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
        // A single such message poisons an otherwise priced session too.
        let mixed = format!(
            "{TURN_ENDED}{}",
            TOKENS_WITHOUT_COST
                .lines()
                .find(|l| l.contains("\"role\":\"assistant\""))
                .unwrap()
                .replace("\"id\":\"c0eb4fa9\"", "\"id\":\"zz00zz00\"")
        );
        assert!(matches!(
            reported_cost(&mixed),
            ReportedCost::Unavailable { .. }
        ));
    }

    #[test]
    fn an_errored_message_with_zero_tokens_is_neither_a_reading_nor_a_hazard() {
        // The error fixture ends on a `stopReason: error` message with all-zero usage:
        // it must not flip the session to Unavailable, and the priced turns before it
        // still sum.
        match reported_cost(HARD_ERROR) {
            ReportedCost::Usd(usd) => assert!((usd - FIXTURE_USD).abs() < 1e-9, "{usd}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn session_peak_is_the_largest_total_tokens_of_any_turn() {
        // Five turns: 3052, 2891, 3226, 3604, 3797 — the last happens to be the
        // largest here, but it is a max, not a last (checked below).
        assert_eq!(session_peak(TURN_ENDED), Some(3797));
        let reordered: String = {
            let mut lines: Vec<&str> = TURN_ENDED.lines().collect();
            lines.reverse();
            lines.join("\n")
        };
        assert_eq!(session_peak(&reordered), Some(3797));
        assert_eq!(session_peak(""), None);
        // Zero-token (errored) messages never read as a peak of 0.
        let only_error = HARD_ERROR.lines().last().unwrap();
        assert_eq!(session_peak(only_error), None);
    }

    #[test]
    fn turn_state_reads_the_four_states_off_real_tails() {
        assert_eq!(turn_state(TURN_ENDED), TurnState::TurnEnded);
        assert_eq!(turn_state(IN_TOOL_CALL), TurnState::InToolCall);
        assert_eq!(turn_state(AWAITING), TurnState::AwaitingAssistant);
        // A trailing hard error is not a finished turn, and not "awaiting" either:
        // leave it alone, the death/error path says it.
        assert_eq!(turn_state(HARD_ERROR), TurnState::Unknown);
        assert_eq!(turn_state(""), TurnState::Unknown);
        assert_eq!(turn_state("{\"type\":\"session\"}\n"), TurnState::Unknown);
    }

    #[test]
    fn turn_ended_only_on_a_settled_assistant_message() {
        assert!(turn_ended(TURN_ENDED));
        assert!(!turn_ended(IN_TOOL_CALL));
        assert!(!turn_ended(AWAITING));
        assert!(!turn_ended(HARD_ERROR));
    }

    #[test]
    fn a_clipped_tail_is_conservative() {
        // Only the last three lines of the tool-call fixture: the `toolCall` opener is
        // in view, its result is not — still in a tool call.
        let lines: Vec<&str> = IN_TOOL_CALL.lines().collect();
        let tail = lines[lines.len() - 1..].join("\n");
        assert_eq!(turn_state(&tail), TurnState::InToolCall);
        // Only the trailing toolResult of the awaiting fixture: its opener fell
        // outside the window, it closes nothing, and the assistant owes a reply.
        let lines: Vec<&str> = AWAITING.lines().collect();
        let tail = lines[lines.len() - 1..].join("\n");
        assert_eq!(turn_state(&tail), TurnState::AwaitingAssistant);
    }

    #[test]
    fn an_aborted_message_is_not_a_finished_turn() {
        let aborted = TURN_ENDED
            .lines()
            .last()
            .unwrap()
            .replace("\"stopReason\":\"stop\"", "\"stopReason\":\"aborted\"");
        assert_eq!(turn_state(&aborted), TurnState::Unknown);
        assert!(
            hard_error(&aborted).is_none(),
            "an abort is the human's, not an error"
        );
    }

    #[test]
    fn hard_error_is_the_trailing_error_message_and_a_retry_clears_it() {
        let msg = hard_error(HARD_ERROR).expect("a trailing stopReason=error is a hard error");
        assert!(msg.contains("Failed to get response"), "{msg}");
        assert!(hard_error(TURN_ENDED).is_none());
        // pi auto-retries: an error followed by a settled turn is recovered.
        let recovered = format!("{HARD_ERROR}{}\n", TURN_ENDED.lines().last().unwrap());
        assert!(hard_error(&recovered).is_none());
        assert!(turn_ended(&recovered));
        // No `errorMessage` ⇒ a generic label, never `None`.
        let bare = r#"{"type":"message","id":"x","message":{"role":"assistant","content":[],"stopReason":"error"}}"#;
        assert_eq!(hard_error(bare).as_deref(), Some("pi assistant error"));
    }

    #[test]
    fn tolerant_of_torn_lines() {
        let torn = format!("{{not json\n{TURN_ENDED}garbage\n");
        assert!(turn_ended(&torn));
        assert!(matches!(reported_cost(&torn), ReportedCost::Usd(_)));
        assert_eq!(session_peak(&torn), Some(3797));
    }

    #[test]
    fn session_dir_name_matches_pis_encoding() {
        // Measured directory names under ~/.pi/agent/sessions/ (0.85.1).
        assert_eq!(
            session_dir_name(Path::new("/tmp/pi-probe")),
            "--tmp-pi-probe--"
        );
        assert_eq!(
            session_dir_name(Path::new(
                "/tmp/pdo-b4kko9SV/repo/.pdo/runs/20260905-150733-7a89837/nodes/piagent/iter-1"
            )),
            "--tmp-pdo-b4kko9SV-repo-.pdo-runs-20260905-150733-7a89837-nodes-piagent-iter-1--"
        );
        assert_eq!(
            session_dir_name(Path::new("/home/llenoir")),
            "--home-llenoir--"
        );
        // Only ONE leading slash is dropped, `:` is encoded too, `.` is kept.
        assert_eq!(session_dir_name(Path::new("//a:b/.c")), "---a-b-.c--");
    }

    #[test]
    fn resolve_by_id_globs_the_imposed_identity_inside_the_cwd_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("sessions");
        let wd = Path::new("/work/shared-worktree");
        let dir = store.join(session_dir_name(wd));
        std::fs::create_dir_all(&dir).unwrap();
        // Two nodes in the same worktree: two files, one directory (#473 has no
        // equivalent here — the identity is in the file name).
        let a = dir.join("2026-09-05T15-07-33-921Z_sid-a.jsonl");
        let b = dir.join("2026-09-05T15-07-34-000Z_sid-b.jsonl");
        std::fs::write(&a, TURN_ENDED).unwrap();
        std::fs::write(&b, IN_TOOL_CALL).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a session").unwrap();
        assert_eq!(resolve_by_id(&store, wd, "sid-a"), Some(a));
        assert_eq!(resolve_by_id(&store, wd, "sid-b"), Some(b));
        // Unknown id, empty id, unknown cwd: none — never a newest-mtime guess.
        assert_eq!(resolve_by_id(&store, wd, "sid-c"), None);
        assert_eq!(resolve_by_id(&store, wd, ""), None);
        assert_eq!(
            resolve_by_id(&store, Path::new("/elsewhere"), "sid-a"),
            None
        );
        // A prefix match is not an identity match.
        assert_eq!(resolve_by_id(&store, wd, "a"), None);
    }

    #[test]
    fn the_turn_end_extension_signals_completion_on_agent_settled_without_exiting() {
        assert!(TURN_END_EXTENSION_TS.contains("pi.on(\"agent_settled\""));
        assert!(TURN_END_EXTENSION_TS.contains("[\"complete\", \"--auto\"]"));
        assert!(TURN_END_EXTENSION_TS.contains("export default function"));
        // Never `agent_end` / `turn_end` (pi may still retry), never a process exit.
        assert!(!TURN_END_EXTENSION_TS.contains("\"agent_end\""));
        assert!(!TURN_END_EXTENSION_TS.contains("\"turn_end\""));
        assert!(!TURN_END_EXTENSION_TS.contains("process.exit"));
        assert!(!TURN_END_EXTENSION_TS.contains("ctx.shutdown"));
        assert_eq!(TURN_END_EXTENSION_SUFFIX, ".turn-end.ts");
    }
}
