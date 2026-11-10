//! The Runs a Trigger produces, through the run context's public use cases: a
//! fire is a creation the trigger context has already gated (`Runs::start_run`),
//! whose Run the extensions stamp, before its first Node, with the Trigger as
//! its Acteur. The Projets of a Trigger are matched as those of its Runs.

use std::collections::HashMap;
use std::sync::Arc;

use pdo_kernel::Actor;
use pdo_run::application::create_run::{CreateRunRequest, TargetRepoInput};
use pdo_run::domain::state::SandboxMode;
use pdo_run::Runs;
use pdo_workspace::domain::provisioning::ProvisioningRules;
use tracing::warn;

use crate::application::ports::{BoxFuture, RunFromTrigger, TriggerProjects, TriggerRuns};
use crate::domain::trigger::Trigger;

/// The Runs of the run context.
pub struct RunsOfTriggers(pub Arc<Runs>);

impl TriggerRuns for RunsOfTriggers {
    /// A finished Run drops out of the count and frees a slot, which is what makes
    /// "max simultaneous live" hold with no extra state to track.
    fn live_count<'a>(&'a self, trigger_id: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            // The Résumés (ADR-0084): no Run's log is replayed.
            match self.0.store().summaries().await {
                Ok(summaries) => summaries
                    .iter()
                    .filter(|run| {
                        run.triggered_by.as_deref() == Some(trigger_id) && run.status.is_live()
                    })
                    .count(),
                Err(e) => {
                    warn!("trigger scheduler: failed to load the runs: {e}");
                    0
                }
            }
        })
    }

    fn start(&self, fire: RunFromTrigger) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            // `start_run` stamps the Run with its `created_by`, the Trigger,
            // before its first Node.
            match self.0.start_run(request(fire), Vec::new(), None).await {
                Ok(run_id) => Ok(run_id),
                Err((_, body)) => Err(body
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("run creation failed")
                    .to_string()),
            }
        })
    }
}

/// The Projet of a Trigger, checked as the run context checks a launch: a
/// Trigger and the Runs it fires are in the same Projet (ADR-0087).
pub struct ProjectsOfRuns(pub Arc<Runs>);

impl TriggerProjects for ProjectsOfRuns {
    fn refusal<'a>(
        &'a self,
        project: &'a str,
        repo: Option<&'a str>,
    ) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            self.0
                .check_launch_in_project(project, repo)
                .await
                .err()
                .map(|(_, body)| {
                    body.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("this project cannot launch runs")
                        .to_string()
                })
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
        // ADR-0087: a Trigger launches its Runs in its Projet.
        project_id: trigger.project_id.clone(),
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
        // A Trigger carries no extension data.
        extensions: Default::default(),
        source_fetch_error,
        // API 0.8: a fired Run is created by its Trigger, and carries the
        // Trigger's last editor, who answers for it.
        created_by: Some(Actor::Trigger {
            trigger_id: trigger.id.clone(),
        }),
        trigger_editor: trigger.last_editor.clone(),
    }
}

/// A Trigger's stored variable overrides (a JSON object), in the create
/// request's shape; an empty/invalid value yields no overrides.
fn variables(trigger: &Trigger) -> HashMap<String, serde_yaml::Value> {
    serde_json::from_value(trigger.variables.clone()).unwrap_or_default()
}
