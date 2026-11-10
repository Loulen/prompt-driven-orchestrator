//! Create an agentic profile (`POST /settings/agent-profiles`):
//! `agent_profile` × `create`. The extensions stamp their attributes on it.

use pdo_kernel::attributes::{stamp, EntityRef};
use pdo_kernel::{Actor, ResourceKind, Verb};

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
        let mut profile = self
            .profiles
            .create(draft)
            .await
            .map_err(|e| AgentProfileError::Storage(e.0))?;
        let entity = EntityRef::new(ResourceKind::AgentProfile, profile.id.clone());
        profile.attributes = stamp(self.extensions(), actor, entity).await;
        Ok(profile)
    }
}
