//! The reads of the agentic profiles: the list, one profile, and the atomic
//! snapshot a spawn resolves against. They ask no one.

use std::collections::BTreeMap;

use pdo_harness::agent_choice::ResolvedCombo;
use tracing::error;

use crate::application::ports::Stored;
use crate::application::SettingsError;
use crate::domain::agent_profile::{AgentProfile, AgentProfileError};
use crate::Settings;

impl Settings {
    /// Every profile, the reserved `Default` first, then by creation order.
    pub async fn agent_profiles(&self) -> Result<Vec<AgentProfile>, SettingsError> {
        self.profiles.list().await.map_err(|e| {
            error!("failed to list agent profiles: {e}");
            SettingsError::Storage(e.0)
        })
    }

    pub async fn agent_profile(&self, id: &str) -> Result<AgentProfile, SettingsError> {
        match self.profiles.get(id).await {
            Ok(Some(profile)) => Ok(profile),
            Ok(None) => Err(AgentProfileError::NotFound.into()),
            Err(e) => Err(SettingsError::Storage(e.0)),
        }
    }

    /// Every profile's `id → combo`, in one read.
    pub async fn agent_profile_snapshot(&self) -> Stored<BTreeMap<String, ResolvedCombo>> {
        self.profiles.snapshot().await
    }
}
