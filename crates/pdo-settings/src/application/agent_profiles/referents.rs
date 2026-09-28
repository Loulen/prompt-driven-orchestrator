//! Who names an agentic profile (`GET /settings/agent-profiles/{id}/referents`):
//! the instance tier, the Projets, the Triggers and the nodes of the instance's
//! pipelines. A read: it asks no one.

use crate::application::SettingsError;
use crate::domain::agent_profile::AgentProfileError;
use crate::domain::referents::{names_profile, AgentProfileReferents};
use crate::Settings;

impl Settings {
    pub async fn agent_profile_referents(
        &self,
        id: &str,
    ) -> Result<AgentProfileReferents, SettingsError> {
        if !matches!(self.profiles.get(id).await, Ok(Some(_))) {
            return Err(AgentProfileError::NotFound.into());
        }
        let instance = self
            .config
            .get()
            .await
            .ok()
            .is_some_and(|config| names_profile(config.agent_choice.as_ref(), id));
        Ok(AgentProfileReferents {
            profile_id: id.to_string(),
            instance,
            projects: self.referents.projects(id).await,
            triggers: self.referents.triggers(id).await,
            pipelines: self.referents.pipelines(id),
            runs: Vec::new(),
        })
    }
}
