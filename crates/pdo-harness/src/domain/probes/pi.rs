//! `pi`'s capabilities.

use super::*;

/// The `pi` capabilities (#705/#707/#708, story #702; ADR-0051/0052/0043/0063) —
/// **six** present, **one** declared absent with its motive:
///
/// - **cost**: a **reported** cost of constant **1.0** — pi writes `usage.cost.total`
///   in dollars on every assistant message, from its embedded catalogue; PDO sums
///   it, deduplicated by entry id, and never re-derives from tokens
///   ([`crate::domain::pi_session::reported_cost`]). A message with tokens but no cost
///   makes the node's total unavailable, never `$0` (ADR-0052 §2 amended);
/// - **transcript**: the session JSONL, by the identity PDO imposed, globbed inside
///   the working directory's folder (`adapters::transcripts::pi_session_by_id`);
/// - **end of turn**: the injected `agent_settled` extension as the primary
///   substrate (ADR-0043 applied through the `{settings}` hole — see
///   [`turn_end_injection`]), the session tail's `stopReason` as the sweep's fallback;
/// - **context usage**: the per-message `usage.totalTokens` peak, read against the
///   catalogue's context window (#705);
/// - **hard error**: pi stays resident after a `stopReason: "error"`, so its exit
///   code is no verdict ([`HarnessProbes::exit_code_is_verdict`] is `false`) and the
///   session text is ([`crate::domain::pi_session::hard_error`]);
/// - the **usage-limit menu anchor** is absent, explicitly: the probe is
///   informational, its anchor is claude's wording, and it triggers no recovery
///   (ADR-0012, ADR-0051 §"Limites");
/// - the **staging set**: `.pi/agent/` minus `sessions/`, plus two env vars, no
///   autonomy fixup ([`PI_STAGING_SET`], #708, ADR-0063).
///
/// `subagent_transcripts` stays at the trait's default (empty): pi's session store is
/// flat per cwd (one file per session id, no nesting under a session to enumerate).
pub(super) struct PiProbes;

impl HarnessProbes for PiProbes {
    /// A **reported** cost (ADR-0052) of constant 1.0: already in dollars.
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::ReportedByConstant)
    }
    /// The session names the model and the provider on every message and the
    /// thinking level on its `thinking_level_change` events (measured 0.85.1,
    /// #736) — both values observed.
    fn observed_identity_source(&self) -> Option<ObservedIdentitySource> {
        Some(ObservedIdentitySource::ModelAndEffort)
    }
    fn transcript_reading(&self) -> Option<TranscriptReading> {
        Some(TranscriptReading::PiSessionMessages)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::PiJsonlById)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::PiAgentSettled)
    }
    /// Declared absent, explicitly (see above).
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        None
    }
    /// [`PI_STAGING_SET`]: `.pi/agent/` minus `sessions/`, two env vars, no fixup
    /// (#708, ADR-0063).
    fn staging_set(&self) -> Option<StagingSet> {
        Some(PI_STAGING_SET)
    }
    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::PiSessionPeak)
    }
    fn steering_source(&self) -> Option<SteeringSource> {
        Some(SteeringSource::PiSessionUserMessages)
    }

    /// The sweep's fallback: a tail whose last substantial record is an assistant
    /// message with `stopReason` `stop`/`length` and no tool call pending. A trailing
    /// `error` is never a finished turn.
    fn classify_turn_ended(&self, tail: &str) -> bool {
        crate::domain::pi_session::turn_ended(tail)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::domain::pi_session::session_peak(text)
    }

    fn steering_count(&self, text: &str) -> Option<u32> {
        crate::domain::steering::pi_steering_count(text)
    }

    /// pi survives a hard model failure (it stays resident and says
    /// `stopReason: "error"`), so its exit is not a verdict — the session text is.
    fn exit_code_is_verdict(&self) -> bool {
        false
    }

    /// The session's trailing `stopReason: "error"` with its `errorMessage`.
    fn classify_hard_error(&self, tail: &str) -> Option<String> {
        crate::domain::pi_session::hard_error(tail)
    }

    /// The per-message model (and provider) plus the thinking level in force,
    /// grouped by model × effort (#736) — the observed identity of one session
    /// (#737).
    fn observed_identities(&self, text: &str) -> Vec<ObservedIdentity> {
        crate::domain::pi_session::observed_usage(text)
            .into_iter()
            .map(|usage| ObservedIdentity {
                model: usage.model,
                effort: usage.effort,
                provider: usage.provider,
            })
            .collect()
    }
}

/// The single `pi` instance handed out by [`probes_for`]. Zero-sized.
pub(super) static PI_PROBES: PiProbes = PiProbes;
