//! The model × effort slices of one execution and of the leftover sessions
//! (ADR-0065).

use super::*;

/// The claude transcript fold's per-model output plus the execution's own
/// availability — what a derived (token × price table) fold can hand the slice
/// builder, bundled so the builder's signature stays readable.
pub struct DerivedFold {
    pub models: Option<BTreeMap<String, ModelUsage>>,
    pub usd: Option<f64>,
    pub form: Option<CostForm>,
    pub partial: bool,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
}

/// The **model × effort slices** of one execution (ADR-0065): the observed
/// slices the source reported when it named any, else the claude fold's
/// per-message models, else the requested id in fallback — with the
/// execution's own availability carried along. A harness with no cost source
/// gets NO slices (absent from « By model », never a "default of X" bucket),
/// and so does a mute source with no requested model: nothing is invented.
pub fn execution_model_slices(
    execution: &Execution,
    observed: Vec<ModelEffortSlice>,
    derived: DerivedFold,
) -> Vec<ModelEffortSlice> {
    if !pdo_harness::probes::can_cost(&execution.harness) {
        return Vec::new();
    }
    // The source spoke (#736): `pi`'s per-message model/provider/effort,
    // `copilot`'s usage-point model/effort — the observed values win verbatim.
    if !observed.is_empty() {
        return observed;
    }
    if pdo_harness::probes::observed_identity_source(&execution.harness).is_none()
        && execution.requested_model.is_none()
    {
        return Vec::new();
    }
    if let Some(models) = derived.models {
        if !models.is_empty() {
            // Observed: one slice per verbatim id the source named, the effort
            // requested (`claude`'s source never writes it — ADR-0065 §1).
            return models
                .into_iter()
                .map(|(model, usage)| ModelEffortSlice {
                    model,
                    model_observed: true,
                    effort: execution.requested_effort.clone(),
                    effort_observed: execution.requested_effort.as_ref().map(|_| false),
                    provider: None,
                    usd: Some(usage.usd),
                    estimated: true,
                    partial: !usage.unpriced.is_empty(),
                    executions: 1,
                    unpriced_models: usage.unpriced.into_iter().collect(),
                    missing_reasons: Vec::new(),
                })
                .collect();
        }
    }
    // Source mute for the model (or no readable transcript): the requested id
    // in fallback, carrying the execution's own readability.
    execution
        .requested_model
        .clone()
        .map(|model| {
            vec![ModelEffortSlice {
                model,
                model_observed: false,
                effort: execution.requested_effort.clone(),
                effort_observed: execution.requested_effort.as_ref().map(|_| false),
                provider: None,
                usd: derived.usd,
                estimated: derived.form == Some(CostForm::Derived) && derived.usd.is_some(),
                partial: derived.partial,
                executions: 1,
                unpriced_models: derived.unpriced_models,
                missing_reasons: derived.missing_reasons,
            }]
        })
        .unwrap_or_default()
}

/// Leftover (infrastructure / unassigned) cost, ventilated by its observed
/// models. No effort: those sessions have no node and no startup event, so they
/// land in the "not set" bucket (ADR-0065).
pub fn leftover_model_slices(
    models: &BTreeMap<String, ModelUsage>,
    executions: i64,
) -> Vec<ModelEffortSlice> {
    models
        .iter()
        .map(|(model, usage)| ModelEffortSlice {
            model: model.clone(),
            model_observed: true,
            effort: None,
            effort_observed: None,
            provider: None,
            usd: Some(usage.usd),
            estimated: true,
            partial: !usage.unpriced.is_empty(),
            executions,
            unpriced_models: usage.unpriced.iter().cloned().collect(),
            missing_reasons: Vec::new(),
        })
        .collect()
}
