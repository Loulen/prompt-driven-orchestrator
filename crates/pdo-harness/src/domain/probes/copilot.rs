//! `copilot`'s capabilities.

use super::*;

/// The `copilot` capabilities (#615, ADR-0051/0052) — **three** present (a
/// reported cost, a transcript resolution, an end-of-turn substrate), **two**
/// declared absent with their motive:
///
/// - the **usage-limit menu anchor** is absent: it is an informational probe whose
///   own documentation admits the textual anchor drifts each version, and it
///   triggers no recovery (ADR-0012) — a second harness declaring it absent
///   degrades nothing actionable (ADR-0051 §"Limites");
/// - the **staging set** is absent: configuring `copilot` is a documented
///   prerequisite, not PDO code (ADR-0063 / CONTEXT.md § "Harnais agentique").
///
/// The three present capabilities dispatch to `copilot`'s own implementation — its
/// event journal, never `claude`'s cwd-keyed JSONL store.
///
/// `subagent_transcripts` is left at the trait's default (empty) — investigated,
/// not assumed: `copilot`'s journal declares no delegate/subagent event kind
/// ([`crate::domain::copilot_journal`]'s module doc lists all four it does carry), and its
/// store is flat (`<store>/<session-id>/events.jsonl`, no directory nesting under
/// a session to enumerate).
pub(super) struct CopilotProbes;

impl HarnessProbes for CopilotProbes {
    /// A **reported** cost (ADR-0052): the harness counts itself, PDO converts by a
    /// published constant. Distinct from `claude`'s derived cost — never through the
    /// price table.
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::ReportedByConstant)
    }
    /// The journal names the model and the reasoning effort at session opening,
    /// moves them with the `session.model_change` events, and names the model at
    /// every usage point (measured 1.0.83, #736) — both values observed.
    fn observed_identity_source(&self) -> Option<ObservedIdentitySource> {
        Some(ObservedIdentitySource::ModelAndEffort)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::CopilotEventsJsonl)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::CopilotEventJournal)
    }
    // usage_limit_anchor / staging_set stay `None` — declared absent (see above).

    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::CopilotJournalPeak)
    }
    fn steering_source(&self) -> Option<SteeringSource> {
        Some(SteeringSource::CopilotJournalUserMessages)
    }

    /// A journal trailing on a `session.error` (a hard failure the harness exits 0
    /// on) is **not** a finished turn, so an errored node is never auto-completed.
    fn classify_turn_ended(&self, tail: &str) -> bool {
        crate::domain::copilot_journal::turn_ended(tail)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::domain::context_peak::copilot_session_peak(text)
    }

    fn steering_count(&self, text: &str) -> Option<u32> {
        crate::domain::steering::copilot_steering_count(text)
    }

    /// The model in effect at the journal's usage points, with the reasoning
    /// effort then in force (#736) — the observed identity of one journal (#737).
    fn observed_identities(&self, text: &str) -> Vec<ObservedIdentity> {
        crate::domain::copilot_journal::observed_usage(text)
            .into_iter()
            .map(|usage| ObservedIdentity {
                model: usage.model,
                effort: usage.effort,
                provider: None,
            })
            .collect()
    }

    /// `copilot` exits 0 on a hard model failure (ADR-0052), so its exit is not a
    /// verdict — the journal is.
    fn exit_code_is_verdict(&self) -> bool {
        false
    }

    /// The journal's trailing `session.error` — the signal the harness's exit code
    /// (zero) cannot give.
    fn classify_hard_error(&self, tail: &str) -> Option<String> {
        crate::domain::copilot_journal::hard_error(tail)
    }
}

/// The single `copilot` instance handed out by [`probes_for`]. Zero-sized.
pub(super) static COPILOT_PROBES: CopilotProbes = CopilotProbes;
