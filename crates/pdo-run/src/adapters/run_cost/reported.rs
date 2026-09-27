//! A reported cost (`copilot`, `pi`) for one execution, and the model × effort
//! slices its source names (ADR-0052 §2, ADR-0065 §1).

use super::*;


/// A **reported** cost for one execution (ADR-0052 §2), read from the store its
/// harness's probes resolve, together with the observed model × effort slices
/// the same source names (#736, ADR-0065 §1): `(usd, unavailable_reason,
/// reported_in_usd, slices)`.
///
/// - `copilot`: the journal's cumulative `totalNanoAiu` × the published constant
///   ([`pdo_harness::copilot_journal`]); converted, so not `reported_in_usd`. The
///   slices come from the same journal — the model in effect at the usage
///   points, with the reasoning effort then in force.
/// - `pi`: the sum of `usage.cost.total` over the session's assistant messages,
///   **already in dollars** (constant 1.0, [`pdo_harness::pi_session`]) — `reported_in_usd`.
///   A message with tokens but no cost makes the figure **unavailable** with its
///   reason ("catalogue absent"), never `$0`. The slices come from the same
///   session — the per-message model and provider, the thinking level in effect.
///
/// Attribution is by the imposed session identity (both harnesses); `pi` also needs
/// the working directory, whose folder its file lives in. A reading exists
/// mid-session for both, so a live node has a cost rather than a "—" until its reap.
pub fn reported_execution(
    execution: &Execution,
    stores: &HarnessStores,
) -> (Option<f64>, Option<String>, bool, Vec<ModelEffortSlice>) {
    let Some(session_id) = execution.session_id.as_deref() else {
        return (
            None,
            Some("missing session identity".to_string()),
            false,
            Vec::new(),
        );
    };
    if execution.harness == pdo_harness::registry::COPILOT {
        let text =
            std::fs::read_to_string(stores.copilot.join(session_id).join("events.jsonl")).ok();
        let usd = text
            .as_deref()
            .and_then(pdo_harness::copilot_journal::reported_cost_usd);
        let slices = text
            .as_deref()
            .map(observed_slices_from_copilot)
            .unwrap_or_default();
        return (
            usd,
            usd.is_none()
                .then(|| "no reported cost reading".to_string()),
            false,
            slices,
        );
    }
    if execution.harness == pdo_harness::registry::PI {
        let Some(working_dir) = execution.working_dir.as_deref() else {
            return (
                None,
                Some("missing node identity".to_string()),
                true,
                Vec::new(),
            );
        };
        let text = pdo_harness::adapters::transcripts::resolve_transcript(
            &execution.harness,
            &stores.pi,
            working_dir,
            Some(session_id),
        )
        .and_then(|path| std::fs::read_to_string(path).ok());
        let slices = text.as_deref().map(observed_slices_from_pi);
        return match text.as_deref().map(pdo_harness::pi_session::reported_cost) {
            Some(pdo_harness::pi_session::ReportedCost::Usd(usd)) => {
                (Some(usd), None, true, slices.unwrap_or_default())
            }
            Some(pdo_harness::pi_session::ReportedCost::Unavailable { reason }) => {
                // The models are still observed even though the cost is not:
                // the slices keep their identity and say why the dollars are "—".
                let slices = slices
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut slice| {
                        slice.usd = None;
                        slice.missing_reasons = vec![reason.clone()];
                        slice
                    })
                    .collect();
                (None, Some(reason), true, slices)
            }
            Some(pdo_harness::pi_session::ReportedCost::NoReading) | None => (
                None,
                Some("no reported cost reading".to_string()),
                true,
                Vec::new(),
            ),
        };
    }
    (
        None,
        Some("harness has no reported cost".to_string()),
        false,
        Vec::new(),
    )
}

/// The observed [`ModelEffortSlice`]s of one `pi` session (#736): one per
/// model × thinking level the source names, cost reported (already dollars —
/// never an estimate of this fold, hence `estimated: false`).
pub fn observed_slices_from_pi(text: &str) -> Vec<ModelEffortSlice> {
    pdo_harness::pi_session::observed_usage(text)
        .into_iter()
        .map(|usage| ModelEffortSlice {
            model: usage.model,
            model_observed: true,
            effort: usage.effort.clone(),
            effort_observed: usage.effort.as_ref().map(|_| true),
            usd: Some(usage.usd),
            estimated: false,
            partial: false,
            executions: 1,
            unpriced_models: Vec::new(),
            missing_reasons: Vec::new(),
            provider: usage.provider,
        })
        .collect()
}

/// The observed [`ModelEffortSlice`]s of one `copilot` journal (#736): one per
/// model × reasoning effort the usage points name, cost reported by the
/// published constant (never an estimate, hence `estimated: false`).
pub fn observed_slices_from_copilot(text: &str) -> Vec<ModelEffortSlice> {
    pdo_harness::copilot_journal::observed_usage(text)
        .into_iter()
        .map(|usage| ModelEffortSlice {
            model: usage.model,
            model_observed: true,
            effort: usage.effort.clone(),
            effort_observed: usage.effort.as_ref().map(|_| true),
            usd: Some(usage.usd),
            estimated: false,
            partial: false,
            executions: 1,
            unpriced_models: Vec::new(),
            missing_reasons: Vec::new(),
            provider: None,
        })
        .collect()
}

