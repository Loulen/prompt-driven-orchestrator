//! The `sandbox` context's routes: the staging profiles over SQLite, the home
//! they stage under, and who names a profile (the tiers the settings context
//! reads). The Run's side of its sandbox (its context, its merge-back) is the
//! run context's (`pdo_run::application::sandbox`).

use std::sync::Arc;

use axum::Router;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_sandbox::adapters::host::EnvHome;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_sandbox::application::profiles::SandboxProfiles;
use pdo_settings::adapters::contexts::SandboxTierReferents;

use super::ContextResources;

pub(super) fn router(r: &ContextResources, host: Arc<dyn ExtensionHost>) -> Router {
    let profiles = SandboxProfiles::new(
        Arc::new(SqliteProfiles::new(r.db.clone())),
        host,
        Arc::new(EnvHome::new(r.sandbox_home_override.clone())),
        Arc::new(SandboxTierReferents { db: r.db.clone() }),
    );
    pdo_sandbox::adapters::http::router(Arc::new(profiles))
}
