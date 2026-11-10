//! `POST /settings/harnesses/{name}/refresh`: probe a harness's model catalogue
//! again, even at the same binary version, and tell every open page. It runs
//! the harness's binary on demand: `instance_settings` × `refresh_catalogue`.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use serde_json::Value;

use super::SettingsError;
use crate::Settings;

impl Settings {
    /// The harness's entry of `harness_descriptors.harnesses`, probed now. A
    /// probe of it already running is awaited, never doubled.
    pub async fn refresh_harness_catalogue(
        &self,
        actor: &Actor,
        name: &str,
    ) -> Result<Value, SettingsError> {
        self.authorize(
            actor,
            &Operation::new(
                ResourceKind::InstanceSettings,
                Verb::Action("refresh_catalogue".into()),
            )
            .on(name),
        )?;
        let entry = self
            .harnesses
            .refresh(name)
            .await
            .ok_or_else(|| SettingsError::NotFound(format!("no harness `{name}`")))?;
        self.catalogue_events.catalogue_refreshed(name);
        Ok(entry)
    }
}
