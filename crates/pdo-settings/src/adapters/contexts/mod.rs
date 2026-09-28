//! The settings context and the others, both ways.
//!
//! - `upstream`: the ports the other contexts read their instance settings
//!   through (the run, review, trigger and platform contexts'), implemented
//!   over the stored tier and the agentic profiles, read fresh at every call so
//!   a `PUT /settings` takes effect without a restart.
//! - here: what the settings context reads of the others, behind its own
//!   ports — the staging profiles, the version check, who names a profile.

mod referents;
mod upstream;

use std::sync::Arc;

use pdo_platform::Platform;
use pdo_sandbox::application::ports::SandboxProfileRepository;

pub use referents::ContextReferents;
pub use upstream::{
    RunInstanceSettings, SettingsOfReview, SettingsOfTriggers, UpdateCheckOfPlatform,
};

use crate::application::ports::{BoxFuture, StagingProfiles, UpdateChecks};

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
