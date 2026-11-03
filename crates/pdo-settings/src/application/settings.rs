//! The settings context, assembled: its ports.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};

use super::ports::{
    AgentProfileStore, AppearanceStore, CatalogueEvents, Harnesses, InstanceConfigStore,
    ProfileReferents, SettingsSection, StagingProfiles, TelemetryReloads, UpdateChecks,
};

/// Every port the context needs, as the composition root plugs them in.
pub struct SettingsPorts {
    /// The stored tier of the instance defaults.
    pub config: Arc<dyn InstanceConfigStore>,
    pub profiles: Arc<dyn AgentProfileStore>,
    /// The Apparence of each Actor.
    pub appearances: Arc<dyn AppearanceStore>,
    /// The sections of the page, one per context that discloses something on
    /// it. The page is their union, plus the instance tiers the settings
    /// context owns itself (the agentic profile and the skills selection).
    pub sections: Vec<Arc<dyn SettingsSection>>,
    pub harnesses: Arc<dyn Harnesses>,
    /// Told when a harness's catalogue was probed again.
    pub catalogue_events: Arc<dyn CatalogueEvents>,
    pub staging_profiles: Arc<dyn StagingProfiles>,
    pub update_checks: Arc<dyn UpdateChecks>,
    pub telemetry: Arc<dyn TelemetryReloads>,
    pub referents: Arc<dyn ProfileReferents>,
    /// Asked before every mutation.
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The context, assembled: its use cases over its ports.
pub struct Settings {
    pub(crate) config: Arc<dyn InstanceConfigStore>,
    pub(crate) profiles: Arc<dyn AgentProfileStore>,
    pub(crate) appearances: Arc<dyn AppearanceStore>,
    pub(crate) sections: Vec<Arc<dyn SettingsSection>>,
    pub(crate) harnesses: Arc<dyn Harnesses>,
    pub(crate) catalogue_events: Arc<dyn CatalogueEvents>,
    pub(crate) staging_profiles: Arc<dyn StagingProfiles>,
    pub(crate) update_checks: Arc<dyn UpdateChecks>,
    pub(crate) telemetry: Arc<dyn TelemetryReloads>,
    pub(crate) referents: Arc<dyn ProfileReferents>,
    extensions: Arc<dyn ExtensionHost>,
}

impl Settings {
    pub fn new(ports: SettingsPorts) -> Arc<Self> {
        let SettingsPorts {
            config,
            profiles,
            appearances,
            sections,
            harnesses,
            catalogue_events,
            staging_profiles,
            update_checks,
            telemetry,
            referents,
            extensions,
        } = ports;
        Arc::new(Self {
            config,
            profiles,
            appearances,
            sections,
            harnesses,
            catalogue_events,
            staging_profiles,
            update_checks,
            telemetry,
            referents,
            extensions,
        })
    }

    /// The extension host: the attributes of the profiles.
    pub(crate) fn extensions(&self) -> &dyn ExtensionHost {
        self.extensions.as_ref()
    }

    /// Ask the extensions whether `actor` may run `operation`.
    pub(crate) fn authorize(&self, actor: &Actor, operation: &Operation) -> Result<(), Denied> {
        authorize(self.extensions.as_ref(), actor, operation)
    }
}
