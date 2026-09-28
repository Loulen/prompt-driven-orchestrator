//! The reads of the agentic profiles: the list, one profile, and the atomic
//! snapshot a spawn resolves against. They ask no one.

use std::collections::BTreeMap;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::attributes::{self, AttributeFilter, AttributedList};
use pdo_kernel::ResourceKind;
use tracing::error;

use crate::application::ports::Stored;
use crate::application::SettingsError;
use crate::domain::agent_profile::{AgentProfile, AgentProfileError};
use crate::Settings;

impl Settings {
    /// Every profile carrying the attributes `filter` asks for, the reserved
    /// `Default` first, then by creation order, each with its attributes.
    pub async fn agent_profiles(
        &self,
        filter: AttributeFilter,
    ) -> Result<Vec<AgentProfile>, SettingsError> {
        let profiles = self.profiles.list().await.map_err(|e| {
            error!("failed to list agent profiles: {e}");
            SettingsError::Storage(e.0)
        })?;
        let attributed =
            AttributedList::read(self.extensions(), ResourceKind::AgentProfile, filter).await;
        Ok(profiles
            .into_iter()
            .filter(|profile| attributed.keeps(&profile.id))
            .map(|mut profile| {
                profile.attributes = attributed.of(&profile.id);
                profile
            })
            .collect())
    }

    /// `profile`, with its attributes.
    pub(crate) async fn with_attributes(&self, mut profile: AgentProfile) -> AgentProfile {
        profile.attributes =
            attributes::of_entity(self.extensions(), ResourceKind::AgentProfile, &profile.id).await;
        profile
    }

    pub async fn agent_profile(&self, id: &str) -> Result<AgentProfile, SettingsError> {
        match self.profiles.get(id).await {
            Ok(Some(profile)) => Ok(self.with_attributes(profile).await),
            Ok(None) => Err(AgentProfileError::NotFound.into()),
            Err(e) => Err(SettingsError::Storage(e.0)),
        }
    }

    /// Every profile's `id → combo`, in one read.
    pub async fn agent_profile_snapshot(&self) -> Stored<BTreeMap<String, ResolvedCombo>> {
        self.profiles.snapshot().await
    }
}
