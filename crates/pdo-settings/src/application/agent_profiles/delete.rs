//! Delete an agentic profile (`DELETE /settings/agent-profiles/{id}`):
//! `agent_profile` × `delete` on the profile. The reserved `Default` is refused
//! unconditionally (ADR-0057 ¶3 / #563 AC10), before the store is even asked.
//! Otherwise unconditional: the referents dialog informs the confirmation, the
//! database does not refuse (ADR-0031 §7).

use pdo_kernel::{Actor, Verb};

use super::operation;
use crate::application::SettingsError;
use crate::domain::agent_profile::{AgentProfileError, DEFAULT_PROFILE_ID};
use crate::Settings;

impl Settings {
    pub async fn delete_agent_profile(&self, actor: &Actor, id: &str) -> Result<(), SettingsError> {
        self.authorize(actor, &operation(Verb::Delete, Some(id)))?;
        if id == DEFAULT_PROFILE_ID {
            return Err(AgentProfileError::DefaultUndeletable.into());
        }
        match self.profiles.delete(id).await {
            Ok(true) => Ok(()),
            // A failed delete reads as a missing profile, as it always has.
            Ok(false) | Err(_) => Err(AgentProfileError::NotFound.into()),
        }
    }
}
