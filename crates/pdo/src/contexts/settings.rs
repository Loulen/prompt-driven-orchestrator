//! The `settings` context: the stored tier and the agentic profiles over
//! SQLite, the page's sections — each context's instance defaults, the staging
//! profiles and Docker, the harnesses' catalogues, the price table — under the
//! daemon's home, the platform's version check, and who names a profile (the
//! Projets, the Triggers, the instance pipelines).

use std::sync::Arc;

use pdo_harness::adapters::binary_probe::installed_catalogue_reader;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::Platform;
use pdo_sandbox::adapters::local_runtime;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_settings::adapters::contexts::{
    ContextReferents, PlatformUpdateChecks, SandboxStagingProfiles,
};
use pdo_settings::adapters::sections::{
    HarnessCatalogue, HostHome, PlatformDefaults, PriceTableSection, ReviewDefaults, RunDefaults,
    SandboxSection, SessionDefaults, TriggerDefaults,
};
use pdo_settings::{Settings, SettingsPorts};

use super::{ContextResources, StoredSettings};

pub(super) fn settings(
    r: &ContextResources,
    stored: StoredSettings,
    platform: &Arc<Platform>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Settings> {
    let home = HostHome {
        override_home: r.sandbox_home_override.clone(),
    };
    let profiles = Arc::new(SqliteProfiles::new(r.db.clone()));
    let harnesses = Arc::new(HarnessCatalogue {
        home: home.clone(),
        reader: installed_catalogue_reader(),
    });
    let sandbox = SandboxSection::new(
        profiles.clone(),
        local_runtime(r.docker_cmd_override.as_deref()).docker,
        home.clone(),
    );
    Settings::new(SettingsPorts {
        config: stored.config,
        profiles: stored.profiles,
        sections: vec![
            Arc::new(RunDefaults),
            Arc::new(SessionDefaults),
            Arc::new(TriggerDefaults),
            Arc::new(ReviewDefaults),
            Arc::new(PlatformDefaults),
            Arc::new(sandbox),
            harnesses.clone(),
            Arc::new(PriceTableSection { home }),
        ],
        harnesses,
        staging_profiles: Arc::new(SandboxStagingProfiles(profiles)),
        update_checks: Arc::new(PlatformUpdateChecks(platform.clone())),
        referents: Arc::new(ContextReferents {
            db: r.db.clone(),
            repo_root: r.repo_root.clone(),
        }),
        extensions: host,
    })
}
