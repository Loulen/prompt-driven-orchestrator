//! Create an agentic profile (`POST /settings/agent-profiles`):
//! `agent_profile` × `create`.

use pdo_kernel::{Actor, Verb};

use super::{draft, operation, ProfileRequest};
use crate::application::SettingsError;
use crate::domain::agent_profile::{AgentProfile, AgentProfileError};
use crate::Settings;

impl Settings {
    pub async fn create_agent_profile(
        &self,
        actor: &Actor,
        request: ProfileRequest,
    ) -> Result<AgentProfile, SettingsError> {
        self.authorize(actor, &operation(Verb::Create, None))?;
        let draft = draft(self.profiles.as_ref(), &request, None).await?;
        self.profiles
            .create(draft)
            .await
            .map_err(|e| AgentProfileError::Storage(e.0).into())
    }
}
