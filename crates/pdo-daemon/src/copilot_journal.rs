//! Reading GitHub Copilot CLI's per-session **event journal** (#615, ADR-0052).
//!
//! `copilot` is PDO's second first-party harness. Where `claude` leaves a JSONL
//! transcript indexed by encoded working directory (`crate::run_cost`), `copilot`
//! writes an **event journal** at
//! `<store>/<session-id>/events.jsonl` — indexed by the **session identity PDO
//! imposed** at launch (`--session-id`, `crate::harness_registry::copilot`), with
//! **no** working-directory encoding. That is deliberate: the #473 collision two
//! nodes sharing a worktree suffer under `claude`'s cwd-keyed store has no
//! structural equivalent here, because the session id is unique per node.
//!
//! This module is **pure**: journal text in, three facts out, no I/O, no `$HOME`.
//! Its callers (`crate::harness_probes` for turn-end, `crate::run_cost` for the
//! reported cost) inject the bytes they read from disk.
//!
//! ## The three facts, each measured against a real journal
//!
//! - **End of turn.** The journal carries an explicit `assistant.turn_end` event.
//!   It is the substrate (ADR-0043): it depends on no instance setting, writes
//!   nothing into the user's config, and feeds the liveness sweep. A tail whose
//!   last turn marker is `assistant.turn_end` (and which is not trailed by a hard
//!   error) is a finished turn.
//!
//! - **Hard error.** A `session.error` event carries a hard failure (a model
//!   failure after the retries are exhausted). It matters because **the harness
//!   exits 0 on such a failure** — the exit code is not a verdict; the journal is.
//!   So a tail trailing on a `session.error` is NOT a finished turn: the node must
//!   not be auto-completed as if it had succeeded (it fails visibly when its
//!   session dies, `crate::stale_detector`).
//!
//! - **Reported cost.** `session.usage_checkpoint` (written live, after each turn)
//!   and `session.shutdown` carry a cumulative `totalNanoAiu` — the harness's own
//!   count, in its billing unit (**nano-AIU**, nano AI-credits). PDO converts it to
//!   USD by a **published constant** ([`nano_aiu_to_usd`]), never through the price
//!   table (ADR-0052 §2): the cache buckets do not map onto `claude`'s, and
//!   `copilot`'s `inputTokens` already includes cache, so re-deriving from tokens
//!   would double-count the cache silently. A live checkpoint means a running node
//!   has a cost, not a "—" until its reap.

/// The published conversion constant (ADR-0052 §2). GitHub Copilot bills in **AI
/// credits (AIU)**, one credit worth **one US cent**; the journal reports the
/// cumulative spend in **nano-AIU** (`totalNanoAiu`). So:
///
/// ```text
/// USD = nanoAiu × 1e-9 (AIU per nano-AIU) × 0.01 (USD per AIU) = nanoAiu × 1e-11
/// ```
///
/// This is a **constant**, not an estimate: it does not degrade the honesty of the
/// harness's own figure, and it makes a reported cost additive with a derived one
/// (both in dollars). It is a datum to watch — the billing unit changed once (the
/// premium request ceased to be the only tier — but it is unique and published,
/// where a price table is a catalogue.
const USD_PER_AIU: f64 = 0.01;
/// Nano prefix: 1 AIU = 1e9 nano-AIU.
const NANO_PER_AIU: f64 = 1e9;

/// Convert a cumulative `totalNanoAiu` reading to USD by the published constant
/// (ADR-0052). Pure arithmetic — no price table, so it can never produce an
/// `unpriced_models` signal and never grows an Anthropic price catalogue with a
/// family that does not belong to it.
pub(crate) fn nano_aiu_to_usd(nano_aiu: u64) -> f64 {
    (nano_aiu as f64) / NANO_PER_AIU * USD_PER_AIU
}

/// The reported cost of a session, in USD, read from its event-journal text — or
/// `None` when the journal carries no usage reading yet (a session that has not
/// finished a first turn). Reads the **maximum** `totalNanoAiu` across every
/// `session.usage_checkpoint` / `session.shutdown` event: the field is a running
/// cumulative total, so the largest reading is the latest, whether the session is
/// still live (last checkpoint) or done (shutdown). `None` — not `Some(0.0)` — when
/// absent, so a caller can tell "no reading" from "a reading of zero".
pub(crate) fn reported_cost_usd(journal: &str) -> Option<f64> {
    max_total_nano_aiu(journal).map(nano_aiu_to_usd)
}

/// The maximum `totalNanoAiu` seen across the journal's usage events, or `None`.
/// Tolerant, line-by-line: a torn/invalid JSON line is skipped, never propagated.
fn max_total_nano_aiu(journal: &str) -> Option<u64> {
    let mut max: Option<u64> = None;
    for raw in journal.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty != "session.usage_checkpoint" && ty != "session.shutdown" {
            continue;
        }
        if let Some(nano) = v
            .get("data")
            .and_then(|d| d.get("totalNanoAiu"))
            .and_then(|n| n.as_u64())
        {
            max = Some(max.map_or(nano, |m| m.max(nano)));
        }
    }
    max
}

/// The event types that mark the shape of a turn, in the order they decide a
/// tail's verdict. Only these three are consulted; usage/shutdown/info events that
/// trail a turn do not change whether the turn *ended*.
enum TurnMarker {
    Started,
    Ended,
    Errored,
}

fn turn_marker(ty: &str) -> Option<TurnMarker> {
    match ty {
        "assistant.turn_start" => Some(TurnMarker::Started),
        "assistant.turn_end" => Some(TurnMarker::Ended),
        // A hard error the journal carries — the harness exits 0 on it, so this is
        // the only truthful signal that the turn did NOT complete successfully.
        "session.error" => Some(TurnMarker::Errored),
        _ => None,
    }
}

/// Whether this journal `tail` shows a **finished turn** — `copilot`'s end-of-turn
/// signature (ADR-0043 / #615). True iff the last turn marker in the tail is an
/// `assistant.turn_end`: a trailing `assistant.turn_start` (a turn in flight) or a
/// trailing `session.error` (a hard failure the harness would exit 0 on) both
/// answer `false`, so no node is auto-completed while working, nor mistaken for
/// finished after an error. Usage / shutdown / info events that follow a turn-end
/// are ignored — they do not un-finish the turn.
pub(crate) fn turn_ended(tail: &str) -> bool {
    let mut last: Option<TurnMarker> = None;
    for raw in tail.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if let Some(m) = turn_marker(ty) {
            last = Some(m);
        }
    }
    matches!(last, Some(TurnMarker::Ended))
}

/// Whether this journal `tail` trails on a **hard error** — a `session.error` that
/// is the last turn marker (ADR-0052 / #615). This is the journal-borne error the
/// harness's exit code (zero) cannot report. Callers use it to say the failure
/// *as such*, rather than reading it off a code that lies. `Some(message)` carries
/// the error text (best-effort), `None` when the last marker is not an error.
pub(crate) fn hard_error(tail: &str) -> Option<String> {
    let mut last_error_msg: Option<String> = None;
    let mut last_is_error = false;
    for raw in tail.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match turn_marker(ty) {
            Some(TurnMarker::Errored) => {
                last_is_error = true;
                last_error_msg = v
                    .get("data")
                    .and_then(|d| d.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from);
            }
            Some(_) => {
                last_is_error = false;
                last_error_msg = None;
            }
            None => {}
        }
    }
    if last_is_error {
        Some(last_error_msg.unwrap_or_else(|| "copilot session error".to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures shaped exactly like the measured journal (see the module header).
    const TURN_START: &str = r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#;
    const TURN_END: &str = r#"{"type":"assistant.turn_end","data":{"turnId":"0"}}"#;
    const CHECKPOINT: &str = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":2823580000,"totalPremiumRequests":1}}"#;
    const SHUTDOWN: &str = r#"{"type":"session.shutdown","data":{"shutdownType":"routine","totalNanoAiu":2823580000}}"#;
    const HARD_ERR: &str = r#"{"type":"session.error","data":{"errorType":"query","message":"Failed to get response from the AI model; retried 5 times"}}"#;

    // --- the published-constant conversion (ADR-0052) ---

    #[test]
    fn nano_aiu_converts_by_the_published_constant() {
        // 2 823 580 000 nano-AIU = 2.82358 AIU = 2.82358 cents = $0.0282358.
        assert!((nano_aiu_to_usd(2_823_580_000) - 0.0282358).abs() < 1e-12);
        // 1e11 nano-AIU = 100 AIU = $1.00 (the constant, checked at a round point).
        assert!((nano_aiu_to_usd(100_000_000_000) - 1.0).abs() < 1e-12);
        assert_eq!(nano_aiu_to_usd(0), 0.0);
    }

    #[test]
    fn reported_cost_reads_the_max_total_and_is_none_when_absent() {
        // A live journal: turn-end + a checkpoint → a cost while the node runs.
        let live = format!("{TURN_START}\n{TURN_END}\n{CHECKPOINT}\n");
        assert!((reported_cost_usd(&live).unwrap() - 0.0282358).abs() < 1e-12);
        // A shutdown carries the same cumulative total — the max is unchanged.
        let done = format!("{live}{SHUTDOWN}\n");
        assert!((reported_cost_usd(&done).unwrap() - 0.0282358).abs() < 1e-12);
        // A journal with no usage event yet → None, not Some(0.0).
        assert!(reported_cost_usd(&format!("{TURN_START}\n")).is_none());
        assert!(reported_cost_usd("").is_none());
    }

    #[test]
    fn reported_cost_takes_the_largest_reading_across_growing_checkpoints() {
        // Two turns: the cumulative total grows; the latest (largest) is the cost.
        let cp1 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":1000000000}}"#;
        let cp2 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000}}"#;
        let journal = format!("{TURN_START}\n{TURN_END}\n{cp1}\n{TURN_START}\n{TURN_END}\n{cp2}\n");
        assert!(
            (reported_cost_usd(&journal).unwrap() - nano_aiu_to_usd(5_000_000_000)).abs() < 1e-12
        );
    }

    // --- end of turn ---

    #[test]
    fn turn_end_is_the_last_turn_marker() {
        // A finished turn, whether or not usage/shutdown trail it.
        assert!(turn_ended(&format!("{TURN_START}\n{TURN_END}\n")));
        assert!(turn_ended(&format!(
            "{TURN_START}\n{TURN_END}\n{CHECKPOINT}\n"
        )));
        assert!(turn_ended(&format!(
            "{TURN_START}\n{TURN_END}\n{CHECKPOINT}\n{SHUTDOWN}\n"
        )));
    }

    #[test]
    fn a_turn_in_flight_is_not_ended() {
        // A turn started and not yet ended → the node is working, not finished.
        assert!(!turn_ended(&format!("{TURN_END}\n{TURN_START}\n")));
        // No turn marker at all → not ended.
        assert!(!turn_ended(CHECKPOINT));
        assert!(!turn_ended(""));
    }

    #[test]
    fn a_trailing_hard_error_is_not_a_finished_turn() {
        // The measured hazard (#615): a hard error the harness exits 0 on. A prior
        // successful turn-end must NOT make this read as finished.
        let journal = format!("{TURN_START}\n{TURN_END}\n{TURN_START}\n{HARD_ERR}\n");
        assert!(
            !turn_ended(&journal),
            "an errored turn is not a finished turn"
        );
    }

    // --- hard error ---

    #[test]
    fn hard_error_is_recognised_from_the_journal_not_the_exit_code() {
        let journal = format!("{TURN_START}\n{HARD_ERR}\n");
        let msg = hard_error(&journal).expect("a trailing session.error is a hard error");
        assert!(msg.contains("Failed to get response from the AI model"));
    }

    #[test]
    fn a_successful_turn_carries_no_hard_error() {
        assert!(hard_error(&format!("{TURN_START}\n{TURN_END}\n{CHECKPOINT}\n")).is_none());
        // An error followed by a fresh successful turn is cleared (recovered).
        let recovered = format!("{HARD_ERR}\n{TURN_START}\n{TURN_END}\n");
        assert!(hard_error(&recovered).is_none());
    }

    #[test]
    fn tolerant_of_torn_lines() {
        let torn = format!("{{not json\n{TURN_END}\ngarbage\n{CHECKPOINT}\n");
        assert!(turn_ended(&torn));
        assert!(reported_cost_usd(&torn).is_some());
    }
}
