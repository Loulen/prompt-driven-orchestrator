//! Reading GitHub Copilot CLI's per-session **event journal** (#615, ADR-0052).
//!
//! `copilot` is PDO's second first-party harness. Where `claude` leaves a JSONL
//! transcript indexed by encoded working directory (`run_cost`), `copilot`
//! writes an **event journal** at
//! `<store>/<session-id>/events.jsonl` — indexed by the **session identity PDO
//! imposed** at launch (`--session-id`, `crate::domain::registry::copilot`), with
//! **no** working-directory encoding. That is deliberate: the #473 collision two
//! nodes sharing a worktree suffer under `claude`'s cwd-keyed store has no
//! structural equivalent here, because the session id is unique per node.
//!
//! This module is **pure**: journal text in, three facts out, no I/O, no `$HOME`.
//! Its callers (`crate::domain::probes` for turn-end, `run_cost` for the
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
//!   session dies, `stale_detector`).
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
/// (both in dollars). Watch it — the billing unit has changed once already.
const USD_PER_AIU: f64 = 0.01;
const NANO_PER_AIU: f64 = 1e9;

/// Convert a cumulative `totalNanoAiu` reading to USD by the published constant
/// (ADR-0052). Pure arithmetic — no price table, so it can never produce an
/// `unpriced_models` signal and never grows an Anthropic price catalogue with a
/// family that does not belong to it.
pub fn nano_aiu_to_usd(nano_aiu: u64) -> f64 {
    (nano_aiu as f64) / NANO_PER_AIU * USD_PER_AIU
}

/// The reported cost of a session, in USD, read from its event-journal text — or
/// `None` when the journal carries no usage reading yet (a session that has not
/// finished a first turn). Reads the **maximum** `totalNanoAiu` across every
/// `session.usage_checkpoint` / `session.shutdown` event: the field is a running
/// cumulative total, so the largest reading is the latest, whether the session is
/// still live (last checkpoint) or done (shutdown). `None` — not `Some(0.0)` — when
/// absent, so a caller can tell "no reading" from "a reading of zero".
pub fn reported_cost_usd(journal: &str) -> Option<f64> {
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

/// One observed model × effort group of a copilot session (#736, ADR-0065 §1):
/// the model in effect at the session's usage points (its own `totalNanoAiu`
/// deltas — the harness's reported spend, already converted by the published
/// constant) with the reasoning effort then in force.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedUsage {
    pub model: String,
    pub effort: Option<String>,
    /// The group's delta of `totalNanoAiu`, converted to USD.
    pub usd: f64,
}

/// The single model id a usage event's `modelCacheState` names, if any. Several
/// distinct ids in one checkpoint (a change in the middle of a billing window)
/// is an ambiguity the fold refuses to guess out of — `None` sends the delta to
/// the tracked model instead. `auto` is the picker, not a model id.
fn single_cache_model(data: Option<&serde_json::Value>) -> Option<String> {
    let ids: std::collections::BTreeSet<&str> = data?
        .get("modelCacheState")?
        .as_array()?
        .iter()
        .filter_map(|entry| entry.get("modelId").and_then(|m| m.as_str()))
        .filter(|m| !m.is_empty() && *m != "auto")
        .collect();
    if ids.len() == 1 {
        ids.into_iter().next().map(String::from)
    } else {
        None
    }
}

/// The observed model × effort ventilation of one copilot journal (#736): the
/// spend each usage event adds (the `totalNanoAiu` delta — the field is a
/// running cumulative total) attributed to the model in effect at that point,
/// grouped by model × effort in first-appearance order. The model in effect is
/// the session's opening selection (`session.start`) as moved by the
/// `session.model_change` events; the usage point's own `modelCacheState` wins
/// when it names exactly one real model (the resolution of an `auto` picker).
/// A delta whose model stays unresolvable is not attributed to anything —
/// nothing is invented. Pure and tolerant, like every reader here.
pub fn observed_usage(journal: &str) -> Vec<ObservedUsage> {
    let mut model: Option<String> = None;
    let mut effort: Option<String> = None;
    let mut prev_nano: u64 = 0;
    let mut groups: Vec<ObservedUsage> = Vec::new();
    let mut index: std::collections::HashMap<(String, Option<String>), usize> =
        std::collections::HashMap::new();
    for raw in journal.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let data = v.get("data");
        let field = |name: &str| {
            data.and_then(|d| d.get(name))
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        match ty {
            // The chosen model and reasoning effort at session opening (measured
            // 1.0.83: both ride `data` when the session is launched with them).
            "session.start" => {
                model = field("selectedModel").or(model);
                effort = field("reasoningEffort").or(effort);
            }
            // A model change carries the new id and (when it moves) the effort;
            // a change without an effort keeps the one in force.
            "session.model_change" => {
                if let Some(m) = field("newModel") {
                    model = Some(m);
                }
                if let Some(e) = data
                    .and_then(|d| d.get("reasoningEffort"))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                {
                    effort = Some(e.to_string());
                }
            }
            "session.usage_checkpoint" | "session.shutdown" => {
                let Some(nano) = data
                    .and_then(|d| d.get("totalNanoAiu"))
                    .and_then(|n| n.as_u64())
                else {
                    continue;
                };
                let delta = nano.saturating_sub(prev_nano);
                if nano > prev_nano {
                    prev_nano = nano;
                }
                if delta == 0 {
                    continue;
                }
                let Some(attributed) =
                    single_cache_model(data).or_else(|| model.clone().filter(|m| m != "auto"))
                else {
                    continue;
                };
                let key = (attributed.clone(), effort.clone());
                let entry = match index.get(&key) {
                    Some(&i) => &mut groups[i],
                    None => {
                        groups.push(ObservedUsage {
                            model: attributed,
                            effort: effort.clone(),
                            usd: 0.0,
                        });
                        let i = groups.len() - 1;
                        index.insert(key, i);
                        &mut groups[i]
                    }
                };
                entry.usd += nano_aiu_to_usd(delta);
            }
            _ => {}
        }
    }
    groups
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
/// signature (ADR-0043). True iff the LAST turn marker is an `assistant.turn_end`:
/// a trailing `assistant.turn_start` (turn in flight) or a trailing `session.error`
/// (a hard failure the harness exits 0 on) both answer `false`, so no node is
/// auto-completed while working, nor mistaken for finished after an error. Usage /
/// shutdown / info events after a turn-end are ignored — they do not un-finish it.
pub fn turn_ended(tail: &str) -> bool {
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
/// is the last turn marker (ADR-0052). This is the journal-borne error the
/// harness's exit code (zero) cannot report, so callers must not read the failure
/// off that code. `Some(message)` carries the error text (best-effort).
pub fn hard_error(tail: &str) -> Option<String> {
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
        // A checkpoint gives a cost while the node still runs, not only at shutdown.
        let live = format!("{TURN_START}\n{TURN_END}\n{CHECKPOINT}\n");
        assert!((reported_cost_usd(&live).unwrap() - 0.0282358).abs() < 1e-12);
        let done = format!("{live}{SHUTDOWN}\n");
        assert!((reported_cost_usd(&done).unwrap() - 0.0282358).abs() < 1e-12);
        // None, not Some(0.0): "no reading" is not "a reading of zero".
        assert!(reported_cost_usd(&format!("{TURN_START}\n")).is_none());
        assert!(reported_cost_usd("").is_none());
    }

    #[test]
    fn reported_cost_takes_the_largest_reading_across_growing_checkpoints() {
        let cp1 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":1000000000}}"#;
        let cp2 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000}}"#;
        let journal = format!("{TURN_START}\n{TURN_END}\n{cp1}\n{TURN_START}\n{TURN_END}\n{cp2}\n");
        assert!(
            (reported_cost_usd(&journal).unwrap() - nano_aiu_to_usd(5_000_000_000)).abs() < 1e-12
        );
    }

    /// One more test case (#736): the observed model × effort ventilation, cut
    /// from a measured 1.0.83 journal — the opening names model + effort, a
    /// model_change moves the effort, the usage point confirms the model.
    #[test]
    fn observed_usage_names_the_model_and_effort_at_the_usage_points() {
        let start = r#"{"type":"session.start","data":{"selectedModel":"gpt-5.6-sol","reasoningEffort":"medium"}}"#;
        let change = r#"{"type":"session.model_change","data":{"newModel":"gpt-5.6-sol","previousReasoningEffort":"low","reasoningEffort":"high"}}"#;
        let cp1 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":10000000000,"modelCacheState":[{"modelId":"gpt-5.6-sol"}]}}"#;
        let cp2 = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":30000000000,"modelCacheState":[{"modelId":"gpt-5.6-sol"}]}}"#;
        let usage = observed_usage(&format!(
            "{start}\n{TURN_START}\n{TURN_END}\n{cp1}\n{change}\n{TURN_START}\n{TURN_END}\n{cp2}\n{SHUTDOWN}\n"
        ));
        assert_eq!(usage.len(), 2, "one group per effort in force");
        assert_eq!(usage[0].model, "gpt-5.6-sol");
        assert_eq!(usage[0].effort.as_deref(), Some("medium"));
        assert!((usage[0].usd - nano_aiu_to_usd(10_000_000_000)).abs() < 1e-12);
        assert_eq!(usage[1].effort.as_deref(), Some("high"));
        // The shutdown event re-reports the same cumulative total: zero delta.
        assert!((usage[1].usd - nano_aiu_to_usd(20_000_000_000)).abs() < 1e-12);
    }

    #[test]
    fn observed_usage_resolves_an_auto_picker_at_the_usage_point() {
        // The measured hazard: the session opens on `auto` (the picker, not a
        // model); the usage point's modelCacheState names the real model.
        let start = r#"{"type":"session.start","data":{}}"#;
        let initial = r#"{"type":"session.model_change","data":{"cause":"initial_resolution","newModel":"auto","reasoningEffort":null}}"#;
        let cp = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000,"modelCacheState":[{"modelId":"gpt-5.6-sol"}]}}"#;
        let usage = observed_usage(&format!("{start}\n{initial}\n{cp}\n"));
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].model, "gpt-5.6-sol");
        assert_eq!(usage[0].effort, None);
    }

    #[test]
    fn observed_usage_is_empty_without_a_usage_reading_or_a_resolvable_model() {
        // No checkpoints: nothing cost anything, nothing is attributed.
        let start = r#"{"type":"session.start","data":{"selectedModel":"gpt-5.6-sol","reasoningEffort":"low"}}"#;
        assert!(observed_usage(start).is_empty());
        assert!(observed_usage("").is_empty());
        // A checkpoint with an unresolvable model (pure `auto`, no cache state)
        // is not attributed to anything — nothing is invented.
        let auto = r#"{"type":"session.model_change","data":{"newModel":"auto"}}"#;
        let bare = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000}}"#;
        assert!(observed_usage(&format!("{auto}\n{bare}\n")).is_empty());
        // Torn lines are skipped, never propagated.
        let cp = r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":1000000000}}"#;
        assert!(observed_usage(&format!("{auto}\n{{not json\n{cp}\n")).is_empty());
    }

    #[test]
    fn turn_end_is_the_last_turn_marker() {
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
        assert!(!turn_ended(&format!("{TURN_END}\n{TURN_START}\n")));
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
