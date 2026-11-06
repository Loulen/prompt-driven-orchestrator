//! The `settings` context: the stored tier and the agentic profiles over
//! SQLite, the Apparence of each Actor, the page's sections — each context's instance defaults, the staging
//! profiles and Docker, the harnesses' catalogues, the price table — under the
//! daemon's home, the platform's version check, the telemetry export an edit
//! reloads, the bank the instance's skills are checked against, and who names
//! a profile (the Projets, the Triggers, the instance pipelines).

use std::sync::Arc;

use pdo_harness::adapters::binary_probe::installed_catalogue_reader;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_platform::Platform;
use pdo_sandbox::adapters::local_runtime;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_settings::adapters::contexts::{
    BankSkills, ContextReferents, PlatformUpdateChecks, SandboxStagingProfiles, TelemetryOfSettings,
};
use pdo_settings::adapters::sections::{
    HarnessCatalogue, HostHome, PlatformDefaults, PriceTableSection, ReviewDefaults, RunDefaults,
    SandboxSection, SessionDefaults, TelemetryDefaults, TriggerDefaults, WorkspaceDefaults,
};
use pdo_settings::adapters::sqlite::SqliteAppearances;
use pdo_settings::application::ports::CatalogueEvents;
use pdo_settings::{Settings, SettingsPorts};
use tokio::sync::broadcast;

use super::{ContextResources, StoredSettings};

pub(super) fn settings(
    r: &ContextResources,
    stored: StoredSettings,
    platform: &Arc<Platform>,
    telemetry: &Arc<pdo_telemetry::Telemetry>,
    skills: &Arc<pdo_skill::Skills>,
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
        appearances: Arc::new(SqliteAppearances::new(r.db.clone())),
        sections: vec![
            Arc::new(RunDefaults),
            Arc::new(SessionDefaults),
            Arc::new(TriggerDefaults),
            Arc::new(ReviewDefaults),
            Arc::new(PlatformDefaults),
            Arc::new(TelemetryDefaults),
            Arc::new(WorkspaceDefaults),
            Arc::new(sandbox),
            harnesses.clone(),
            Arc::new(PriceTableSection { home }),
        ],
        harnesses,
        catalogue_events: Arc::new(BusCatalogueEvents(r.bus_tx.clone())),
        staging_profiles: Arc::new(SandboxStagingProfiles(profiles)),
        skills: Arc::new(BankSkills(skills.clone())),
        update_checks: Arc::new(PlatformUpdateChecks(platform.clone())),
        telemetry: Arc::new(TelemetryOfSettings(telemetry.clone())),
        referents: Arc::new(ContextReferents {
            db: r.db.clone(),
            repo_root: r.repo_root.clone(),
        }),
        extensions: host,
    })
}

/// A harness's catalogue probed again, announced on the raw channel of the
/// kernel's bus, which `/ws` relays to every open page.
struct BusCatalogueEvents(broadcast::Sender<serde_json::Value>);

impl CatalogueEvents for BusCatalogueEvents {
    fn catalogue_refreshed(&self, harness: &str) {
        // No listener (no page open) is not a failure.
        let _ = self.0.send(serde_json::json!({
            "type": "harness_catalogue_changed",
            "harness": harness,
        }));
    }
}
