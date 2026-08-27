//! Harness-specific context-window **peak** parsing (#585, Stats → Performance).
//!
//! A session's context peak is the maximum "occupancy" any one of its turns
//! reached — never a percentage, never converted through a model's context-window
//! catalogue (explicitly out of scope, see the issue's "Out of Scope" §1/§2). Each
//! harness reports occupancy in its own shape, so each gets its own parser; there
//! is no generic "usage" struct shared across them (ADR-0051: a capability is
//! code, written harness by harness, never a shared heuristic).
//!
//! Both parsers are **pure**: transcript/journal text in, `Option<u64>` tokens
//! out. No I/O, no clock — the caller ([`crate::stats_performance`]) resolves the
//! file and injects its bytes, exactly like [`crate::run_cost`] and
//! [`crate::copilot_journal`] already do for cost.
//!
//! ## Claude (#585 Implementation Decisions §"Pour Claude")
//!
//! A turn's occupancy is `input_tokens` (non-cache) + `cache_read_input_tokens` +
//! `cache_creation_input_tokens` (both the 5m/1h buckets) + `output_tokens` — the
//! same four cache/input buckets [`crate::run_cost`] costs, plus the turn's own
//! output. Unlike cost, which **sums** every line, a context peak takes the
//! **max**: each assistant message already carries the *whole* conversation's
//! current window occupancy (Claude's `input_tokens`/cache fields are not
//! per-message deltas — they are the full context sent for that turn), so the
//! largest single turn IS the session's peak.
//!
//! Messages replayed on resume/compaction are deduplicated by `(message.id,
//! requestId)` before the max is taken — **the same dedup key** `run_cost`'s
//! `aggregate` uses for cost, because a replayed message is not a second, larger
//! turn; counting it twice would let a resume/compaction artefact invent a peak
//! that never happened.
//!
//! ## Copilot (#585 Implementation Decisions §"Pour Copilot")
//!
//! Copilot's `session.usage_checkpoint` / `session.shutdown` events (the same two
//! kinds [`crate::copilot_journal::reported_cost_usd`] reads) carry a `usage`
//! object whose `inputTokens` / `outputTokens` fields are **cumulative since
//! session start** — mirroring `totalNanoAiu`'s own cumulative semantics. Two
//! rules the issue is explicit about:
//!
//! - `inputTokens` **already includes** whatever the journal separately reports
//!   as cache (`cacheReadTokens` / `cacheCreationTokens`), so those two buckets are
//!   read for information only and are **never added again** — doing so would
//!   double-count the cache.
//! - the cumulative counters are converted to their **per-turn contribution**
//!   (this reading minus the previous one) before the max is sought — the
//!   cumulative *total* grows monotonically and would otherwise always crown the
//!   session's LAST turn as its peak, whatever that turn actually occupied.
//!
//! A turn's occupancy is then `Δ inputTokens + Δ outputTokens` for that
//! checkpoint. The session's peak is the max across all such deltas.

use serde_json::Value;

// --- Claude ------------------------------------------------------------------

/// One assistant message's dedup key and context occupancy — the same four
/// cache/input buckets [`crate::run_cost`]'s `Usage` costs, plus output, folded
/// into a single total rather than costed.
struct ClaudeTurn {
    message_id: Option<String>,
    request_id: Option<String>,
    occupancy: u64,
}

/// Parse one Claude transcript line into a [`ClaudeTurn`], or `None` to skip it —
/// tolerant of torn/invalid JSON, `<synthetic>` messages, and non-assistant lines,
/// mirroring `run_cost::parse_line`'s tolerance exactly (a session transcript is
/// external input, never trusted to be well-formed).
fn parse_claude_line(raw: &str) -> Option<ClaudeTurn> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    if v.get("isApiErrorMessage").and_then(|b| b.as_bool()) == Some(true) {
        return None;
    }
    let msg = v.get("message")?;
    let model = msg.get("model").and_then(|m| m.as_str())?;
    if model == "<synthetic>" {
        return None;
    }
    let usage = msg.get("usage")?;
    let field = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let input = field("input_tokens");
    let output = field("output_tokens");
    let cache_read = field("cache_read_input_tokens");
    let (cache_create_5m, cache_create_1h) = match usage.get("cache_creation") {
        Some(cc) => (
            cc.get("ephemeral_5m_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            cc.get("ephemeral_1h_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
        ),
        None => (field("cache_creation_input_tokens"), 0),
    };
    let occupancy = input + output + cache_read + cache_create_5m + cache_create_1h;
    if occupancy == 0 {
        return None;
    }
    Some(ClaudeTurn {
        message_id: msg.get("id").and_then(|x| x.as_str()).map(String::from),
        request_id: v
            .get("requestId")
            .and_then(|x| x.as_str())
            .map(String::from),
        occupancy,
    })
}

/// The session's context peak, in tokens — the maximum per-turn occupancy across
/// every **deduplicated** assistant message in `text` (a Claude JSONL transcript,
/// main session or subagent — the parser is the same either way, ADR-0051's "a
/// capability is proper to the harness", not to the role). `None` when the
/// transcript carries no readable usage at all — never `Some(0)`, so an absent
/// reading is never mistaken for a session that used no context.
pub(crate) fn claude_session_peak(text: &str) -> Option<u64> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter_map(parse_claude_line)
        .filter(|turn| {
            match &turn.message_id {
                Some(id) => seen.insert((id.clone(), turn.request_id.clone())),
                // No id to dedup on: never collapsed with another line.
                None => true,
            }
        })
        .map(|turn| turn.occupancy)
        .max()
}

/// The `[start, end]` wall-clock span covered by every top-level `"timestamp"`
/// field found across `text`'s lines (a Claude JSONL transcript) — the "reliable
/// start/end bounds" #585's subagent-duration user story asks for, since a
/// subagent has no `node_started`/`node_completed` pair of its own (only its
/// parent Node does). Both timestamps are the transcript's own RFC-3339 strings,
/// unparsed here: the caller ([`crate::stats_performance`]) already owns
/// `chrono` diffing for every other duration in Performance, so this stays a
/// pure string min/max, not a second date library entry point.
///
/// `None` when not a single line carries a readable `timestamp` — the caller
/// reports a motivated absence rather than inventing a duration (#585 explicitly
/// forbids inventing a subagent duration without reliable bounds).
pub(crate) fn claude_transcript_time_span(text: &str) -> Option<(String, String)> {
    let mut min: Option<String> = None;
    let mut max: Option<String> = None;
    for raw in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let Some(ts) = value.get("timestamp").and_then(|t| t.as_str()) else {
            continue;
        };
        if min.as_deref().is_none_or(|m| ts < m) {
            min = Some(ts.to_string());
        }
        if max.as_deref().is_none_or(|m| ts > m) {
            max = Some(ts.to_string());
        }
    }
    min.zip(max)
}

// --- Copilot -------------------------------------------------------------------

/// One `usage` reading off a Copilot journal event — cumulative since session
/// start, exactly like `totalNanoAiu`.
struct CopilotReading {
    input: u64,
    output: u64,
}

fn parse_copilot_usage(value: &Value) -> Option<CopilotReading> {
    let usage = value.get("data")?.get("usage")?;
    let input = usage.get("inputTokens").and_then(|v| v.as_u64())?;
    let output = usage
        .get("outputTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Some(CopilotReading { input, output })
}

/// The session's context peak, in tokens — the maximum per-turn occupancy across
/// every `session.usage_checkpoint` / `session.shutdown` reading in `journal`.
/// Each reading is **cumulative**, so occupancy is the delta from the previous
/// reading (the first reading's delta is against a zero baseline); `inputTokens`
/// already includes whatever the journal separately reports as cache, so only
/// `inputTokens` and `outputTokens` are read — `cacheReadTokens` /
/// `cacheCreationTokens`, if present, are never added a second time. `None` when
/// the journal carries no usage reading at all.
pub(crate) fn copilot_session_peak(journal: &str) -> Option<u64> {
    let mut prev_input = 0u64;
    let mut prev_output = 0u64;
    let mut peak: Option<u64> = None;
    for raw in journal.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let ty = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty != "session.usage_checkpoint" && ty != "session.shutdown" {
            continue;
        }
        let Some(reading) = parse_copilot_usage(&value) else {
            continue;
        };
        // Cumulative counters never legitimately shrink; a shrink would only mean
        // a new session reused the same journal path — clamp at 0 rather than
        // underflow or invent a negative turn.
        let delta =
            reading.input.saturating_sub(prev_input) + reading.output.saturating_sub(prev_output);
        prev_input = reading.input;
        prev_output = reading.output;
        peak = Some(peak.map_or(delta, |p: u64| p.max(delta)));
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Claude --------------------------------------------------------------

    fn claude_line(
        id: &str,
        request: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_5m: u64,
    ) -> String {
        serde_json::json!({
            "type": "assistant",
            "requestId": request,
            "message": {
                "id": id,
                "model": "claude-opus-4-8",
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": cache_read,
                    "cache_creation": { "ephemeral_5m_input_tokens": cache_5m, "ephemeral_1h_input_tokens": 0 }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn claude_peak_is_the_largest_turns_four_bucket_sum_plus_output() {
        // Three turns, non-monotonic sizes: the peak is turn 2's sum
        // (400+50+300+0 = 750), not the last turn nor a running total.
        let text = format!(
            "{}\n{}\n{}\n",
            claude_line("m1", "r1", 100, 20, 50, 0),
            claude_line("m2", "r2", 400, 50, 300, 0),
            claude_line("m3", "r3", 100, 10, 100, 0),
        );
        assert_eq!(claude_session_peak(&text), Some(750));
    }

    #[test]
    fn claude_dedup_keeps_the_replayed_message_from_inflating_the_peak() {
        // The same (message.id, requestId) written twice (resume/compaction
        // replay, ADR-0022) must count once — the peak must not double it.
        let one_turn = claude_line("m1", "r1", 400, 50, 300, 0);
        let replayed = format!("{one_turn}\n{one_turn}\n");
        assert_eq!(claude_session_peak(&replayed), Some(750));
        // A second, larger turn after the replay still wins on its own merits.
        let text = format!("{replayed}{}\n", claude_line("m2", "r2", 900, 10, 0, 0));
        assert_eq!(claude_session_peak(&text), Some(910));
    }

    #[test]
    fn claude_peak_is_absent_without_message_id_dedup_key_but_still_counted() {
        // A line without `message.id` has no dedup key: it is never collapsed
        // with another line (never seen twice in practice, but must not panic
        // or silently vanish).
        let raw = serde_json::json!({
            "type": "assistant",
            "requestId": "r1",
            "message": { "model": "claude-opus-4-8", "usage": { "input_tokens": 10, "output_tokens": 5 } }
        })
        .to_string();
        assert_eq!(claude_session_peak(&format!("{raw}\n")), Some(15));
    }

    #[test]
    fn claude_peak_is_none_on_no_readable_usage() {
        assert_eq!(claude_session_peak(""), None);
        assert_eq!(claude_session_peak("not json\n"), None);
        // A synthetic message (compaction summary) carries no real usage.
        let synthetic = serde_json::json!({
            "type": "assistant",
            "message": { "model": "<synthetic>", "usage": { "input_tokens": 999 } }
        })
        .to_string();
        assert_eq!(claude_session_peak(&format!("{synthetic}\n")), None);
    }

    #[test]
    fn claude_peak_reused_for_a_subagent_transcript() {
        // #585: a subagent transcript is read by the exact same parser as the
        // parent's — the peak has no notion of "role", only of turns.
        let text = claude_line("sub-1", "r1", 200, 30, 0, 0);
        assert_eq!(claude_session_peak(&format!("{text}\n")), Some(230));
    }

    // --- Claude time span ------------------------------------------------------

    fn claude_line_at(id: &str, ts: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ts,
            "requestId": "r",
            "message": { "id": id, "model": "claude-opus-4-8", "usage": { "input_tokens": 1 } }
        })
        .to_string()
    }

    #[test]
    fn claude_time_span_is_the_min_and_max_timestamp_out_of_order() {
        // Lines out of chronological order (a subagent transcript is not
        // guaranteed sorted by wall clock) — min/max, not first/last line.
        let text = format!(
            "{}\n{}\n{}\n",
            claude_line_at("m1", "2026-01-01T00:00:05Z"),
            claude_line_at("m2", "2026-01-01T00:00:01Z"),
            claude_line_at("m3", "2026-01-01T00:00:09Z"),
        );
        assert_eq!(
            claude_transcript_time_span(&text),
            Some((
                "2026-01-01T00:00:01Z".to_string(),
                "2026-01-01T00:00:09Z".to_string()
            ))
        );
    }

    #[test]
    fn claude_time_span_degenerates_to_a_single_instant_for_one_line() {
        let text = claude_line_at("m1", "2026-01-01T00:00:05Z");
        assert_eq!(
            claude_transcript_time_span(&format!("{text}\n")),
            Some((
                "2026-01-01T00:00:05Z".to_string(),
                "2026-01-01T00:00:05Z".to_string()
            ))
        );
    }

    #[test]
    fn claude_time_span_is_none_without_a_single_readable_timestamp() {
        assert_eq!(claude_transcript_time_span(""), None);
        assert_eq!(claude_transcript_time_span("not json\n"), None);
        let no_ts = serde_json::json!({
            "type": "assistant",
            "message": { "id": "m1", "model": "claude-opus-4-8", "usage": { "input_tokens": 1 } }
        })
        .to_string();
        assert_eq!(claude_transcript_time_span(&format!("{no_ts}\n")), None);
    }

    // --- Copilot ---------------------------------------------------------------

    fn checkpoint(input: u64, output: u64, cache_read: u64) -> String {
        serde_json::json!({
            "type": "session.usage_checkpoint",
            "data": {
                "totalNanoAiu": 1,
                "usage": {
                    "inputTokens": input,
                    "outputTokens": output,
                    "cacheReadTokens": cache_read,
                    "cacheCreationTokens": 0
                }
            }
        })
        .to_string()
    }

    #[test]
    fn copilot_peak_converts_cumulative_counters_to_a_per_turn_contribution() {
        // Two turns: cumulative inputTokens grows 500 -> 1300 (turn 2 alone added
        // 800), outputTokens grows 100 -> 260 (turn 2 added 160). Turn 1's own
        // contribution (500 + 100 = 600) is smaller than turn 2's (800 + 160 =
        // 960) — the max must be turn 2's contribution, not the final cumulative
        // total (1300 + 260 = 1560), which a naive "read the last checkpoint"
        // would wrongly report.
        let journal = format!(
            "{}\n{}\n",
            checkpoint(500, 100, 0),
            checkpoint(1300, 260, 0)
        );
        assert_eq!(copilot_session_peak(&journal), Some(960));
    }

    #[test]
    fn copilot_peak_never_double_counts_cache_already_inside_input_tokens() {
        // `inputTokens` already includes the cache; a naive parser that also adds
        // `cacheReadTokens` would inflate the peak past what actually occupied the
        // window. A single turn: inputTokens=1000 (which already covers the 400
        // reported as cache) + outputTokens=50 => peak must be 1050, not 1450.
        let journal = checkpoint(1000, 50, 400);
        assert_eq!(copilot_session_peak(&format!("{journal}\n")), Some(1050));
    }

    #[test]
    fn copilot_peak_first_checkpoint_deltas_against_a_zero_baseline() {
        let journal = checkpoint(700, 80, 0);
        assert_eq!(copilot_session_peak(&format!("{journal}\n")), Some(780));
    }

    #[test]
    fn copilot_peak_reads_shutdown_as_a_reading_too() {
        let shutdown = serde_json::json!({
            "type": "session.shutdown",
            "data": { "totalNanoAiu": 1, "usage": { "inputTokens": 300, "outputTokens": 20 } }
        })
        .to_string();
        assert_eq!(copilot_session_peak(&format!("{shutdown}\n")), Some(320));
    }

    #[test]
    fn copilot_peak_is_none_on_no_readable_usage() {
        assert_eq!(copilot_session_peak(""), None);
        assert_eq!(copilot_session_peak("garbage\n"), None);
        let no_usage =
            serde_json::json!({ "type": "assistant.turn_end", "data": { "turnId": "0" } })
                .to_string();
        assert_eq!(copilot_session_peak(&format!("{no_usage}\n")), None);
    }

    #[test]
    fn copilot_peak_tolerant_of_torn_lines() {
        let torn = format!("{{not json\n{}\ngarbage\n", checkpoint(500, 100, 0));
        assert_eq!(copilot_session_peak(&torn), Some(600));
    }
}
