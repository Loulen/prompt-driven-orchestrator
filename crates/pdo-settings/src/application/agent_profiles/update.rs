//! Edit (and/or rename) an agentic profile (`PUT /settings/agent-profiles/{id}`):
//! `agent_profile` × `update` on the profile. The reserved `Default` included —
//! it stays modifiable and renamable (ADR-0057 ¶3). A full replace of the
//! editable fields, not a sparse patch: a `Custom` choice and a profile share
//! one complete-combination shape.

use pdo_kernel::{Actor, Verb};

use super::{draft, operation, ProfileRequest};
use crate::application::SettingsError;
use crate::domain::agent_profile::{AgentProfile, AgentProfileError};
use crate::Settings;

impl Settings {
    pub async fn update_agent_profile(
        &self,
        actor: &Actor,
        id: &str,
        request: ProfileRequest,
    ) -> Result<AgentProfile, SettingsError> {
        self.authorize(actor, &operation(Verb::Update, Some(id)))?;
        if !matches!(self.profiles.get(id).await, Ok(Some(_))) {
            return Err(AgentProfileError::NotFound.into());
        }
        let draft = draft(self.profiles.as_ref(), &request, Some(id)).await?;
        match self.profiles.replace(id, draft).await {
            Ok(Some(profile)) => Ok(self.with_attributes(profile).await),
            Ok(None) => Err(AgentProfileError::NotFound.into()),
            Err(e) => Err(AgentProfileError::Storage(e.0).into()),
        }
    }
}
