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
use pdo_skill::domain::selection::{foreign_skill_refusal, SkillRef};
use pdo_skill::Skills;

pub use project_tiers::{project_skills, ProjectTiers};
pub use referents::ContextReferents;
pub use sandbox_referents::SandboxTierReferents;
pub use skill_referents::SkillTierReferents;
pub use upstream::{
    ClonesFolderOfWorkspace, PublicUrlOfPlatform, RunInstanceSettings, SettingOfTelemetry,
    SettingsOfReview, SettingsOfTriggers, UpdateCheckOfPlatform,
};

use crate::application::ports::{
    BoxFuture, SkillBank, StagingProfiles, TelemetryReloads, UpdateChecks,
};

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

/// The skill context's bank: the instance's selection only gains skills of
/// the bank (`Skills::foreign_skill`), as a Projet's and a Trigger's do.
pub struct BankSkills(pub Arc<Skills>);

impl SkillBank for BankSkills {
    fn refusal<'a>(
        &'a self,
        current: &'a [SkillRef],
        next: &'a [SkillRef],
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            let foreign = self
                .0
                .foreign_skill(current, next)
                .await
                .map_err(|e| e.to_string())?;
            Ok(foreign.as_deref().map(foreign_skill_refusal))
        })
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
