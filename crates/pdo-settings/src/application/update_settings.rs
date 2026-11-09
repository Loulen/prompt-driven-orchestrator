//! `PUT /settings`: persist the stored tier of one or more knobs, then answer
//! the recomputed page, so the UI shows the new effective value and source
//! without a second round-trip (ADR-0015). `instance_settings` × `update`.
//!
//! Validation is fail-fast and whole: a refused field stores nothing, not even
//! a legitimate sibling edit riding in the same body.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_telemetry::domain::setting::{merge_headers, OtlpHeaderEdit};
use serde_json::Value;

use super::SettingsError;
use crate::domain::edit::{
    check_ranges, normalize_clones_folder, normalize_public_url, parse_edit,
    unknown_manager_profile,
};
use crate::domain::instance_config::UpdateInstanceConfig;
use crate::Settings;

impl Settings {
    pub async fn update(&self, actor: &Actor, body: Value) -> Result<Value, SettingsError> {
        self.authorize(
            actor,
            &Operation::new(ResourceKind::InstanceSettings, Verb::Update),
        )?;
        let mut edit = parse_edit(body).map_err(SettingsError::Invalid)?;
        check_ranges(&edit).map_err(SettingsError::Invalid)?;
        normalize_public_url(&mut edit).map_err(SettingsError::Invalid)?;
        normalize_clones_folder(&mut edit).map_err(SettingsError::Invalid)?;
        self.check_references(&edit).await?;
        self.resolve_kept_headers(&mut edit).await?;

        let update_check = edit.update_check;
        let telemetry = edit.touches_telemetry();
        let stored = self
            .config
            .update(edit)
            .await
            .map_err(|e| SettingsError::Storage(e.0))?;
        self.catalogue_events.settings_changed();
        // #697: re-enabling the check runs one at once, detached — the section
        // reads "Not checked yet since re-enabling." until it lands. Turning it
        // off needs nothing: the read path already answers `null` when off.
        if update_check == Some(true) {
            self.update_checks.check_now();
        }
        // ADR-0090: the export follows the setting without a restart.
        if telemetry {
            self.telemetry.reload();
        }
        self.view_of(&stored).await
    }

    /// A header sent without a value keeps the stored one: resolve them all,
    /// so the store writes values only.
    async fn resolve_kept_headers(
        &self,
        edit: &mut UpdateInstanceConfig,
    ) -> Result<(), SettingsError> {
        let Some(headers) = edit.telemetry_otlp_headers.take() else {
            return Ok(());
        };
        let stored = self
            .config
            .get()
            .await
            .map_err(|e| SettingsError::Storage(e.0))?;
        let merged = merge_headers(&stored.telemetry_otlp_headers, headers)
            .map_err(SettingsError::Invalid)?;
        edit.telemetry_otlp_headers = Some(
            merged
                .into_iter()
                .map(|(name, value)| OtlpHeaderEdit {
                    name,
                    value: Some(value),
                })
                .collect(),
        );
        Ok(())
    }

    /// What names something else must name something that exists: the default
    /// sandbox a staging profile, the manager pin an agentic profile, the
    /// default harness a harness, the skills selection skills of the bank. A
    /// default that resolves to nothing would break every Run falling through
    /// to it, so it is refused at registration.
    async fn check_references(&self, edit: &UpdateInstanceConfig) -> Result<(), SettingsError> {
        if let Some(next) = edit.skills.as_deref().filter(|next| !next.is_empty()) {
            let current = self
                .config
                .get()
                .await
                .map_err(|e| SettingsError::Storage(e.0))?
                .skills;
            if let Some(refusal) = self
                .skills
                .refusal(&current, next)
                .await
                .map_err(SettingsError::Storage)?
            {
                return Err(SettingsError::Invalid(refusal));
            }
        }
        if let Some(sandbox) = edit.default_sandbox.as_deref() {
            // "" = clear sentinel; anything else must be `off` or a staging
            // profile that RESOLVES — the same gate as the Trigger surfaces.
            self.staging_profiles
                .validate(sandbox)
                .await
                .map_err(SettingsError::Invalid)?;
        }
        if let Some(pin) = edit.manager_profile.as_deref().filter(|p| !p.is_empty()) {
            // "" = back to « Follow the Run ». The tombstone of a dangling pin
            // then only ever arises from a LATER delete or rename.
            match self.profiles.find_by_name(pin).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let known: Vec<String> = self
                        .profiles
                        .list()
                        .await
                        .map(|profiles| profiles.into_iter().map(|p| p.name).collect())
                        .unwrap_or_default();
                    return Err(SettingsError::Invalid(unknown_manager_profile(pin, &known)));
                }
                Err(e) => {
                    return Err(SettingsError::Invalid(format!(
                        "cannot check agent profile `{pin}`: {e}"
                    )))
                }
            }
        }
        if let Some(harness) = edit.default_harness.as_deref().filter(|h| !h.is_empty()) {
            self.harnesses
                .check_default(harness)
                .map_err(SettingsError::Invalid)?;
        }
        Ok(())
    }
}
