//! The Runs a Trigger produces, through the run context's public use cases: a
//! fire is a creation the trigger context has already gated (`Runs::start_run`),
//! whose Run the extensions stamp with the Trigger as its Acteur.

use std::collections::HashMap;
use std::sync::Arc;

use pdo_kernel::Actor;
use pdo_run::application::create_run::{CreateRunRequest, TargetRepoInput};
use pdo_run::domain::projection::project;
use pdo_run::domain::state::SandboxMode;
use pdo_run::Runs;
use pdo_workspace::domain::provisioning::ProvisioningRules;
use tracing::warn;

use crate::application::ports::{BoxFuture, RunFromTrigger, TriggerRuns};
use crate::domain::trigger::Trigger;

/// The Runs of the run context.
pub struct RunsOfTriggers(pub Arc<Runs>);

impl TriggerRuns for RunsOfTriggers {
    /// A finished Run drops out of the count and frees a slot, which is what makes
    /// "max simultaneous live" hold with no extra state to track.
    fn live_count<'a>(&'a self, trigger_id: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let store = self.0.store();
            let run_ids = match store.run_ids().await {
                Ok(ids) => ids,
                Err(e) => {
                    warn!("trigger scheduler: failed to load run ids: {e}");
                    return 0;
                }
            };
            let mut count = 0usize;
            for run_id in run_ids {
                let Ok(events) = store.events(&run_id).await else {
                    continue;
                };
                if let Some(state) = project(&events) {
                    if state.triggered_by.as_deref() == Some(trigger_id) && state.status.is_live() {
                        count += 1;
                    }
                }
            }
            count
        })
    }

    fn start(&self, fire: RunFromTrigger) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            let actor = Actor::Trigger {
                trigger_id: fire.trigger.id.clone(),
            };
            match self.0.start_run(request(fire), Vec::new(), None).await {
                Ok(run_id) => {
                    self.0.stamp_run(&actor, &run_id).await;
                    Ok(run_id)
                }
                Err((_, body)) => Err(body
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("run creation failed")
                    .to_string()),
            }
        })
    }
}

/// The create request of a fire: the Trigger IS the Run template.
fn request(fire: RunFromTrigger) -> CreateRunRequest {
    let RunFromTrigger {
        trigger,
        input,
        source_fetch_error,
    } = fire;
    CreateRunRequest {
        pipeline: trigger.pipeline_id.clone(),
        input,
        variables: variables(&trigger),
        pipeline_id: Some(trigger.pipeline_id.clone()),
        target_repo: trigger.target_repo.clone(),
        // Stored as raw JSON TEXT so the Trigger stays decoupled from the Run's
        // types; a blank/absent/malformed value defers to a mono-repo fire.
        target_repos: trigger
            .target_repos
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| serde_json::from_str::<Vec<TargetRepoInput>>(s).ok())
            .unwrap_or_default(),
        source_branch: trigger.source_branch.clone(),
        name: None,
        triggered_by: Some(trigger.id.clone()),
        // A Run has a single origin, so the Trigger's stored mode IS the request's
        // explicit tier. `parse` is syntactic, so an unknown token reads as a profile
        // NAME and the chokepoint 400s it by name; that message becomes this fire's
        // record reason, visibly red in the Trigger history (ADR-0031 §7).
        sandbox: trigger.sandbox.as_deref().and_then(SandboxMode::parse),
        // The Trigger carries the harness IN its Run template — there is no separate
        // Trigger tier. Both a cron tick and a "Run now" route through here, so they
        // freeze the SAME harness.
        harness: trigger.harness.clone(),
        agent_choice: trigger.agent_choice.clone(),
        // #669: the Trigger's skills ARE the fired Run's Run tier, same as `harness`
        // — a cron tick and a "Run now" freeze the SAME list.
        skills: trigger.skills.clone(),
        // `Some(b)` wins the chokepoint resolution unconditionally, so a fire with
        // `auto_name=false` keeps a stable per-id name.
        auto_name: Some(trigger.auto_name),
        // No Trigger tier for `auto_fail` (ADR-0049): a fire states no run-level
        // preference, so `pdo fail` resolves through project/instance.
        auto_fail: None,
        provisioning: ProvisioningRules::default(),
        source_fetch_error,
    }
}

/// A Trigger's stored variable overrides (a JSON object), in the create
/// request's shape; an empty/invalid value yields no overrides.
fn variables(trigger: &Trigger) -> HashMap<String, serde_yaml::Value> {
    serde_json::from_value(trigger.variables.clone()).unwrap_or_default()
}
