//! The settings context and the others, both ways.
//!
//! - `upstream`: the ports the other contexts read their instance settings
//!   through (the run, review, trigger and platform contexts'), implemented
//!   over the stored tier and the agentic profiles, read fresh at every call so
//!   a `PUT /settings` takes effect without a restart.
//! - here: what the settings context reads of the others, behind its own
//!   ports — the staging profiles, the version check, the telemetry export,
//!   who names a profile.
//! - who names a skill or a staging profile, for the skill and sandbox
//!   contexts: the tiers that name them (instance, Projets, Triggers,
//!   pipelines, Runs) live downstream of both, and the settings context reads
//!   them all. So does the Projet tier of the agentic profile and of the skills
//!   (`project_tiers`), which the workspace stores opaque.

mod project_tiers;
mod referents;
mod sandbox_referents;
mod skill_referents;
mod upstream;

use std::sync::Arc;

use pdo_platform::Platform;
use pdo_sandbox::application::ports::SandboxProfileRepository;

pub use project_tiers::{project_skills, ProjectTiers};
pub use referents::ContextReferents;
pub use sandbox_referents::SandboxTierReferents;
pub use skill_referents::SkillTierReferents;
pub use upstream::{
    OtlpSettingOfTelemetry, RunInstanceSettings, SettingsOfReview, SettingsOfTriggers,
    UpdateCheckOfPlatform,
};

use crate::application::ports::{BoxFuture, StagingProfiles, TelemetryReloads, UpdateChecks};

/// The staging profiles of the sandbox context, through its write-time gate.
pub struct SandboxStagingProfiles(pub Arc<dyn SandboxProfileRepository>);

impl StagingProfiles for SandboxStagingProfiles {
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(pdo_sandbox::application::profiles::validate_ref(
            self.0.as_ref(),
            raw,
        ))
    }
}

/// The platform context's version check.
pub struct PlatformUpdateChecks(pub Arc<Platform>);

impl UpdateChecks for PlatformUpdateChecks {
    fn check_now(&self) {
        self.0.check_for_update_in_background();
    }
}

/// The telemetry context's export.
pub struct TelemetryOfSettings(pub Arc<pdo_telemetry::Telemetry>);

impl TelemetryReloads for TelemetryOfSettings {
    fn reload(&self) {
        let telemetry = self.0.clone();
        tokio::spawn(async move { telemetry.reload().await });
    }
}
