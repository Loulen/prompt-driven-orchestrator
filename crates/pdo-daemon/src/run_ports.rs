//! The ports of the run context that what is still legacy implements: the
//! instance settings and the agentic profiles (the settings context), and the
//! page mounts an archive releases (the platform context, reached through the
//! daemon's state).

use std::collections::BTreeMap;
use std::sync::Weak;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_run::application::ports::{BoxFuture, InstanceDefaults, InstanceSettings, RunPages};

use crate::{agent_profile, instance_config, AppState};

/// The instance settings and the agentic profiles, which the settings context
/// (still in this crate) owns, read fresh at every call.
pub(crate) struct LegacyInstanceSettings {
    pub(crate) db: sqlx::SqlitePool,
}

impl InstanceSettings for LegacyInstanceSettings {
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>> {
        Box::pin(async move {
            let c = instance_config::get(&self.db).await.ok()?;
            Some(InstanceDefaults {
                session_cap: c.session_cap,
                default_model: c.default_model,
                default_harness: c.default_harness,
                default_harness_model: c.default_harness_model,
                autocomplete_turn_end: c.autocomplete_turn_end,
                auto_fail: instance_config::resolve_instance_auto_fail(c.auto_fail),
                agent_choice: c.agent_choice,
                skills: c.skills,
                max_attachments_mb: c.max_attachments_mb,
                default_sandbox: c.default_sandbox,
                default_auto_name: c.default_auto_name,
                manager_enabled: instance_config::resolve_manager_enabled(c.manager_enabled),
                manager_profile: c.manager_profile,
            })
        })
    }

    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>> {
        Box::pin(async move { agent_profile::snapshot(&self.db).await.unwrap_or_default() })
    }

    fn profile_named<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<ResolvedCombo>, String>> {
        Box::pin(async move {
            agent_profile::find_by_name_ci(&self.db, name)
                .await
                .map(|found| found.map(|profile| profile.combo()))
                .map_err(|e| e.to_string())
        })
    }
}

/// The page mounts of the platform context, reached through the daemon's
/// state.
pub(crate) struct LegacyRunPages {
    pub(crate) state: Weak<AppState>,
}

impl RunPages for LegacyRunPages {
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>> {
        Box::pin(async move {
            match self.state.upgrade() {
                Some(state) => {
                    Box::new(state.platform.release_run_pages().hold_owned().await) as Box<dyn Send>
                }
                None => Box::new(()) as Box<dyn Send>,
            }
        })
    }

    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(state) = self.state.upgrade() else {
                return Err("the daemon is shutting down".to_string());
            };
            state
                .platform
                .release_run_pages()
                .execute(run_id)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
}
