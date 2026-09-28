//! The ports the upstream contexts read their instance settings through.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_run::adapters::env::{resolve_instance_auto_fail, resolve_manager_enabled};
use pdo_run::application::ports::{InstanceDefaults, InstanceSettings};

use crate::application::ports::{AgentProfileStore, InstanceConfigStore};

type RunFuture<'a, T> = pdo_run::application::ports::BoxFuture<'a, T>;

/// The instance defaults a Run reads at create, spawn and sweep time, and the
/// agentic profiles it resolves against.
pub struct RunInstanceSettings {
    pub config: Arc<dyn InstanceConfigStore>,
    pub profiles: Arc<dyn AgentProfileStore>,
}

impl InstanceSettings for RunInstanceSettings {
    fn defaults(&self) -> RunFuture<'_, Option<InstanceDefaults>> {
        Box::pin(async move {
            let c = self.config.get().await.ok()?;
            Some(InstanceDefaults {
                session_cap: c.session_cap,
                default_model: c.default_model,
                default_harness: c.default_harness,
                default_harness_model: c.default_harness_model,
                autocomplete_turn_end: c.autocomplete_turn_end,
                auto_fail: resolve_instance_auto_fail(c.auto_fail),
                agent_choice: c.agent_choice,
                skills: c.skills,
                max_attachments_mb: c.max_attachments_mb,
                default_sandbox: c.default_sandbox,
                default_auto_name: c.default_auto_name,
                manager_enabled: resolve_manager_enabled(c.manager_enabled),
                manager_profile: c.manager_profile,
            })
        })
    }

    fn agent_profiles(&self) -> RunFuture<'_, BTreeMap<String, ResolvedCombo>> {
        Box::pin(async move { self.profiles.snapshot().await.unwrap_or_default() })
    }

    fn profile_named<'a>(
        &'a self,
        name: &'a str,
    ) -> RunFuture<'a, Result<Option<ResolvedCombo>, String>> {
        Box::pin(async move {
            self.profiles
                .find_by_name(name)
                .await
                .map(|found| found.map(|profile| profile.combo()))
                .map_err(|e| e.to_string())
        })
    }
}

/// `review_agent_can_resolve`, `stored → env → default(false)`.
pub struct SettingsOfReview(pub Arc<dyn InstanceConfigStore>);

impl pdo_review::application::ports::ReviewSettings for SettingsOfReview {
    fn agent_can_resolve(&self) -> pdo_review::application::ports::BoxFuture<'_, bool> {
        Box::pin(async move {
            let stored = self
                .0
                .get()
                .await
                .ok()
                .and_then(|c| c.review_agent_can_resolve);
            pdo_review::adapters::env::agent_can_resolve_with(stored)
        })
    }
}

/// The Triggers' global pause and the guard timeout.
pub struct SettingsOfTriggers(pub Arc<dyn InstanceConfigStore>);

impl pdo_trigger::application::ports::TriggerSettings for SettingsOfTriggers {
    fn paused(&self) -> pdo_trigger::application::ports::BoxFuture<'_, Result<bool, String>> {
        Box::pin(async move { self.0.triggers_paused().await.map_err(|e| e.0) })
    }

    fn set_paused(
        &self,
        paused: bool,
    ) -> pdo_trigger::application::ports::BoxFuture<'_, Result<(), String>> {
        Box::pin(async move { self.0.set_triggers_paused(paused).await.map_err(|e| e.0) })
    }

    /// `stored → env → default`. Stored is seconds; the env seam is ms.
    fn guard_timeout(&self) -> pdo_trigger::application::ports::BoxFuture<'_, Duration> {
        Box::pin(async move {
            let stored_secs = self
                .0
                .get()
                .await
                .ok()
                .and_then(|c| c.guard_timeout_secs)
                .map(|n| n as u64);
            pdo_trigger::adapters::env::guard_timeout_with(stored_secs)
        })
    }
}

/// The platform's `update_check`, `stored → env → default`.
pub struct UpdateCheckOfPlatform(pub Arc<dyn InstanceConfigStore>);

impl pdo_platform::application::ports::UpdateCheckSetting for UpdateCheckOfPlatform {
    fn enabled(&self) -> pdo_platform::application::ports::BoxFuture<'_, bool> {
        Box::pin(async move {
            let stored = self.0.get().await.ok().and_then(|c| c.update_check);
            pdo_platform::domain::update_check::resolve_update_check(
                stored,
                pdo_platform::adapters::env::env_update_check(),
            )
        })
    }
}
