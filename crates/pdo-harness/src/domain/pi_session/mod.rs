//! Reading pi's per-session **JSONL** (#707, story #702; ADR-0052 amended, ADR-0043
//! applied to a third harness).
//!
//! `pi` is PDO's third first-party harness. Its session lives at
//! `<agent_dir>/sessions/<encoded-cwd>/<timestamp>_<session-id>.jsonl` — a directory
//! **named after the working directory** (like `claude`'s store) but a file **named
//! after the session identity PDO imposed** (`--session-id`, like `copilot`'s
//! journal). So two nodes sharing a worktree write two files in one directory, and
//! the resolution is a glob on `*_<id>.jsonl` inside the cwd's directory
//! (`adapters::transcripts::pi_session_by_id`), never a newest-mtime pick.
//!
//! This module is **pure** where it parses: session text in, facts out, no `$HOME`.
//! The filesystem touch (a directory listing) lives in that adapter, because the
//! file name carries a creation timestamp PDO does not know. Its callers
//! (`crate::domain::probes` for the dispatch, `run_cost` for the reported
//! cost, `stats_performance` for the context peak) inject what they read.
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
//!   `claude`'s four-state vocabulary ([`crate::domain::claude_transcript::TurnState`]) so
//!   `assess_node` reads one verdict shape whatever the harness.
//!
//! - **Primary substrate: the turn-end extension.** The equivalent of `claude`'s
//!   injected `Stop` hook (ADR-0043): [`TURN_END_EXTENSION_TS`], written per node by
//!   the daemon and loaded through `-e` (the descriptor's `{settings}` hole), runs
//!   `pdo complete --auto` on `agent_settled` — the event pi documents as "no
//!   automatic retry, compaction retry or queued continuation remains". It never
//!   exits pi and never blocks the turn.

mod turn;

pub use turn::{hard_error, turn_ended, turn_state};

use crate::domain::claude_transcript::TurnState;
use serde_json::Value;
use std::path::Path;

/// The conversion constant of pi's reported cost (ADR-0052 §2): pi's
/// `usage.cost.total` is **already in US dollars**, so the constant is **1.0** —
/// not an approximation, hence no `~` on the surfaces. Published here (rather than
/// inlined as a no-op) so the form of the cost stays legible: it is *reported*, of
/// constant 1.0, not *derived*.
pub const REPORTED_USD_CONSTANT: f64 = 1.0;

/// File-name suffix of the per-node turn-end extension the daemon writes beside the
/// prompt (`<node>-iter-<n>.turn-end.ts`), the twin of `claude`'s
/// `.settings.json`. TypeScript because pi loads extensions through jiti and its
/// documentation names `.ts` as the extension file format.
pub const TURN_END_EXTENSION_SUFFIX: &str = ".turn-end.ts";

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
pub const TURN_END_EXTENSION_TS: &str = r#"// PDO turn-end extension for pi (#707, ADR-0043 applied to pi).
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
pub fn session_dir_name(working_dir: &Path) -> String {
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

/// The reported cost of one pi session, as `run_cost` folds it.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportedCost {
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

/// One observed model × effort group of a session (#736, ADR-0065 §1): the
/// verbatim model id the assistant messages carry, the provider they name (the
/// tooltip's « via openrouter » — never part of the identity), and the thinking
/// level in effect when they were written (observed from the
/// `thinking_level_change` events). The raw material of the « By model » axis
/// for `pi`.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedUsage {
    pub model: String,
    pub provider: Option<String>,
    pub effort: Option<String>,
    /// The group's `usage.cost.total` sum, already in dollars (constant 1.0,
    /// ADR-0052 §2 amended).
    pub usd: f64,
}

/// The observed model × effort ventilation of one pi session (#736): the
/// deduplicated, token-bearing assistant messages grouped by the verbatim
/// `message.model` × the `thinking_level_change` level in effect, in
/// first-appearance order. Pure and tolerant, like every reader here: a torn
/// line is skipped; a message with zero tokens (an errored call that never
/// reached the model) is ignored; a message without a model is skipped (nothing
/// was observed).
pub fn observed_usage(text: &str) -> Vec<ObservedUsage> {
    let mut effort: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    let mut groups: Vec<ObservedUsage> = Vec::new();
    let mut index: std::collections::HashMap<(String, Option<String>), usize> =
        std::collections::HashMap::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw.trim()) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            // The level in effect from here on; only a written level moves it.
            Some("thinking_level_change") => {
                if let Some(level) = v
                    .get("thinkingLevel")
                    .and_then(|t| t.as_str())
                    .filter(|t| !t.is_empty())
                {
                    effort = Some(level.to_string());
                }
            }
            Some("message") => {
                let Some(msg) = v.get("message") else {
                    continue;
                };
                if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                    continue;
                }
                let Some(model) = msg
                    .get("model")
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
                else {
                    continue;
                };
                let Some(usage) = msg.get("usage") else {
                    continue;
                };
                let field = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                let total_tokens = usage
                    .get("totalTokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or_else(|| {
                        field("input") + field("output") + field("cacheRead") + field("cacheWrite")
                    });
                if total_tokens == 0 {
                    continue;
                }
                if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                    if !seen.insert(id.to_string()) {
                        continue; // replayed entry: dedup, same discipline as the cost
                    }
                }
                let provider = msg
                    .get("provider")
                    .and_then(|p| p.as_str())
                    .filter(|p| !p.is_empty())
                    .map(String::from);
                let usd = usage
                    .get("cost")
                    .and_then(|c| c.get("total"))
                    .and_then(|t| t.as_f64())
                    .unwrap_or(0.0);
                let key = (model.to_string(), effort.clone());
                let entry = match index.get(&key) {
                    Some(&i) => &mut groups[i],
                    None => {
                        groups.push(ObservedUsage {
                            model: model.to_string(),
                            provider: None,
                            effort: effort.clone(),
                            usd: 0.0,
                        });
                        let i = groups.len() - 1;
                        index.insert(key, i);
                        &mut groups[i]
                    }
                };
                entry.usd += usd;
                if entry.provider.is_none() {
                    entry.provider = provider;
                }
            }
            _ => {}
        }
    }
    groups
}

/// The reason a token-bearing message without a cost makes the total unavailable.
/// Named here so `run_cost` and the tests agree on the wording a user reads.
pub const CATALOGUE_ABSENT_REASON: &str = "catalogue absent: pi reported tokens without a cost";

/// The session's reported cost (see [`ReportedCost`]). Pure: a torn line is skipped,
/// never propagated. A message with zero tokens (an errored or aborted call that
/// never reached the model) is neither a reading nor a hazard, and is ignored.
pub fn reported_cost(text: &str) -> ReportedCost {
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
pub fn session_peak(text: &str) -> Option<u64> {
    deduplicated_assistant_usages(text)
        .into_iter()
        .map(|u| u.total_tokens)
        .filter(|t| *t > 0)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures cut from a real `pi` 0.85.1 session PDO launched (cwd anonymised,
    // text bodies trimmed, usage figures verbatim). Beside the catalogue fixtures.
    const TURN_ENDED: &str = include_str!("../../../tests/fixtures/pi-0.85.1/turn_ended.jsonl");
    const IN_TOOL_CALL: &str = include_str!("../../../tests/fixtures/pi-0.85.1/in_tool_call.jsonl");
    const AWAITING: &str =
        include_str!("../../../tests/fixtures/pi-0.85.1/awaiting_assistant.jsonl");
    const HARD_ERROR: &str = include_str!("../../../tests/fixtures/pi-0.85.1/hard_error.jsonl");
    const TOKENS_WITHOUT_COST: &str =
        include_str!("../../../tests/fixtures/pi-0.85.1/tokens_without_cost.jsonl");

    /// The five assistant messages of the fixture, `usage.cost.total` summed by hand.
    const FIXTURE_USD: f64 = 0.004496 + 0.003191 + 0.003654 + 0.005052 + 0.004289;

    /// One more test case (#736): the observed model × effort ventilation of the
    /// fixture — every message carries the model verbatim and the provider, the
    /// thinking level comes from the `thinking_level_change` event before them.
    #[test]
    fn observed_usage_names_the_model_provider_and_thinking_level_per_message() {
        let usage = observed_usage(TURN_ENDED);
        assert_eq!(usage.len(), 1, "one session, one model, one level");
        assert_eq!(usage[0].model, "~anthropic/claude-haiku-latest");
        assert_eq!(usage[0].provider.as_deref(), Some("openrouter"));
        assert_eq!(usage[0].effort.as_deref(), Some("low"));
        assert!(
            (usage[0].usd - FIXTURE_USD).abs() < 1e-9,
            "{:?}",
            usage[0].usd
        );
    }

    #[test]
    fn observed_usage_follows_a_mid_session_thinking_level_change() {
        // A second level change, then a fresh message: the messages after the
        // change land in their own observed group (the axis unfolds per effort).
        let change = r#"{"type":"thinking_level_change","id":"zz","thinkingLevel":"high"}"#;
        let fresh = TURN_ENDED
            .lines()
            .find(|l| l.contains("\"role\":\"assistant\""))
            .unwrap()
            .replace("\"id\":\"c0eb4fa9\"", "\"id\":\"zz00zz00\"");
        let usage = observed_usage(&format!("{TURN_ENDED}\n{change}\n{fresh}\n"));
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].effort.as_deref(), Some("low"));
        assert_eq!(usage[1].effort.as_deref(), Some("high"));
        assert_eq!(usage[1].model, "~anthropic/claude-haiku-latest");
        assert!((usage[0].usd - FIXTURE_USD).abs() < 1e-9);
        // The fresh entry id is its own message — counted once, in the high group.
        assert!((usage[1].usd - 0.004496).abs() < 1e-9, "{:?}", usage[1].usd);
    }

    #[test]
    fn observed_usage_skips_zero_token_errored_and_model_less_messages() {
        // The hard-error fixture's trailing message has zero tokens and the
        // awaiting fixture's last record is not an assistant message.
        assert!(observed_usage(HARD_ERROR.lines().last().unwrap()).is_empty());
        assert!(observed_usage("{not json\n").is_empty());
        assert!(observed_usage("").is_empty());
    }

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
            session_dir_name(Path::new("/home/user")),
            "--home-user--"
        );
        // Only ONE leading slash is dropped, `:` is encoded too, `.` is kept.
        assert_eq!(session_dir_name(Path::new("//a:b/.c")), "---a-b-.c--");
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
