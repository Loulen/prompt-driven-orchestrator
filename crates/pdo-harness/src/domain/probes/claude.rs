//! `claude`'s capabilities.

use super::*;
use crate::domain::claude_transcript::{self, TurnState};

/// The `claude` capabilities — all five, exactly as they are today. This slice is
/// a **dispatch refactor**, not a behaviour change: every method returns the
/// mechanism `claude` has always used.
pub(super) struct ClaudeProbes;

impl HarnessProbes for ClaudeProbes {
    fn cost_source(&self) -> Option<CostSource> {
        Some(CostSource::DerivedFromTranscript)
    }
    /// The transcript names the model per assistant message — the observed
    /// model ADR-0065 reads first. The effort is never written by the source,
    /// so it stays requested (measured 2026-09-07, #733).
    fn observed_identity_source(&self) -> Option<ObservedIdentitySource> {
        Some(ObservedIdentitySource::ModelPerMessage)
    }
    fn transcript_resolution(&self) -> Option<TranscriptResolution> {
        Some(TranscriptResolution::ClaudeJsonl)
    }
    fn turn_end_substrate(&self) -> Option<TurnEndSubstrate> {
        Some(TurnEndSubstrate::ClaudeTranscript)
    }
    fn usage_limit_anchor(&self) -> Option<UsageLimitAnchor> {
        Some(UsageLimitAnchor::ClaudePaneMenu)
    }
    fn staging_set(&self) -> Option<StagingSet> {
        Some(CLAUDE_STAGING_SET)
    }

    fn context_usage_source(&self) -> Option<ContextUsageSource> {
        Some(ContextUsageSource::ClaudeTranscriptPeak)
    }
    fn steering_source(&self) -> Option<SteeringSource> {
        Some(SteeringSource::ClaudeTranscriptTurns)
    }
    /// `claude` is the only harness with a confirmed nested-subagent convention
    /// today (#585): `<project_root>/<encoded_cwd>/<session_id>/subagents/`.
    fn subagent_layout(&self) -> Option<SubagentLayout> {
        Some(SubagentLayout::ClaudeSessionSubagentsDir)
    }

    fn classify_turn_ended(&self, tail: &str) -> bool {
        claude_transcript::parse_turn_state(tail) == TurnState::TurnEnded
    }

    fn detect_usage_limit(&self, pane: &str) -> bool {
        claude_transcript::detect_usage_limit(pane)
    }

    fn context_peak(&self, text: &str) -> Option<u64> {
        crate::domain::context_peak::claude_session_peak(text)
    }

    fn steering_count(&self, text: &str) -> Option<u32> {
        crate::domain::steering::claude_steering_count(text)
    }

    /// The verbatim `message.model` ids the transcript's own assistant messages
    /// carry (same tolerance as the cost fold's line parser: API-error and
    /// `<synthetic>` lines are not model readings). The effort is never written
    /// by this source — it stays requested (ADR-0065 §1).
    fn observed_identities(&self, text: &str) -> Vec<ObservedIdentity> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for raw in text.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
                continue;
            };
            if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
                continue;
            }
            if value
                .get("isApiErrorMessage")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                continue;
            }
            let Some(model) = value
                .get("message")
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
                .filter(|m| !m.is_empty() && *m != "<synthetic>")
            else {
                continue;
            };
            if seen.insert(model.to_string()) {
                out.push(ObservedIdentity {
                    model: model.to_string(),
                    effort: None,
                    provider: None,
                });
            }
        }
        out
    }
}

/// The single `claude` instance handed out by [`probes_for`]. Zero-sized, so this
/// `static` costs nothing and needs no lazy init.
pub(super) static CLAUDE_PROBES: ClaudeProbes = ClaudeProbes;
