//! The port of the Agent de support that reads the other contexts: which
//! agent a choice resolves to, over the agentic profiles (settings), the
//! harness registry and the binary probe (harness), and the instance's
//! default models (the Runs').

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_run::Runs;
use pdo_session::application::ports::BoxFuture;
use pdo_session::application::support::{ChoiceRefusal, ResolvedAgent, SupportAgents};

use crate::ports::Late;

/// What the Agents de support read of the daemon.
pub(crate) struct SupportSide {
    pub(crate) runs: Late<Runs>,
    pub(crate) db: sqlx::SqlitePool,
}

impl SupportAgents for SupportSide {
    /// A profile by its id, or the custom combination; its harness resolved
    /// against the descriptor file and its binary found on the harness probe
    /// `PATH`; without a model, the instance's default one for that harness.
    fn resolve<'a>(
        &'a self,
        choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>> {
        Box::pin(async move {
            let (label, harness, model, effort) = match choice {
                AgentChoice::Inherit => return Err(ChoiceRefusal::Inherit),
                AgentChoice::Profile { profile_id } => {
                    let profile = pdo_settings::adapters::sqlite::agent_profiles::get(
                        &self.db,
                        profile_id.trim(),
                    )
                    .await
                    .map_err(|e| ChoiceRefusal::Unreadable(e.to_string()))?
                    .ok_or_else(|| ChoiceRefusal::UnknownProfile(profile_id.clone()))?;
                    (profile.name, profile.harness, profile.model, profile.effort)
                }
                AgentChoice::Custom {
                    harness,
                    model,
                    effort,
                } => (
                    "Custom".to_string(),
                    harness.clone(),
                    model.clone(),
                    effort.clone(),
                ),
            };
            let name = harness.trim();
            let runs = self.runs.get().ok();
            let home_root = runs
                .as_ref()
                .and_then(|runs| pdo_run::application::sandbox::sandbox_home_roots(runs).ok())
                .map(|(home, _)| home)
                .unwrap_or_default();
            let registry = pdo_harness::adapters::descriptor_file::load(&home_root);
            let descriptor = registry
                .resolve(name)
                .ok_or_else(|| ChoiceRefusal::UnknownHarness(name.to_string()))?;
            if !pdo_harness::adapters::binary_path::binary_available(&descriptor.binary) {
                return Err(ChoiceRefusal::NotInstalled(descriptor.name));
            }
            let model = match filled(model) {
                Some(model) => Some(model),
                None => match &runs {
                    Some(runs) => {
                        pdo_run::application::settings::stored_default_harness_models(runs)
                            .await
                            .remove(&descriptor.name)
                    }
                    None => None,
                },
            };
            Ok(ResolvedAgent {
                harness: descriptor,
                model,
                effort: filled(effort),
                label,
            })
        })
    }
}

/// A field that says something: trimmed, `None` when blank.
fn filled(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
