//! `GET /settings`: the instance settings page — per knob, the effective value,
//! the winning tier and every tier's raw value (ADR-0015), plus the facts shown
//! beside them. Each context discloses its own section over the stored tier;
//! this use case only assembles them. A read: it asks no one.

use serde_json::{Map, Value};

use super::SettingsError;
use crate::domain::instance_config::InstanceConfig;
use crate::Settings;

impl Settings {
    /// The page, over the stored tier read now.
    pub async fn view(&self) -> Result<Value, SettingsError> {
        let stored = self
            .config
            .get()
            .await
            .map_err(|e| SettingsError::Storage(e.0))?;
        self.view_of(&stored).await
    }

    /// The page over `stored`: every section's keys, then the instance tiers
    /// the settings context owns — the agentic profile and the skills
    /// selection, as stored — and the time of the last write.
    pub(crate) async fn view_of(&self, stored: &InstanceConfig) -> Result<Value, SettingsError> {
        let mut page = Map::new();
        for section in &self.sections {
            page.extend(
                section
                    .disclose(stored)
                    .await
                    .map_err(SettingsError::Storage)?,
            );
        }
        page.insert("agent_choice".into(), json_of(&stored.agent_choice));
        page.insert("skills".into(), json_of(&stored.skills));
        page.insert("updated_at".into(), json_of(&stored.updated_at));
        Ok(Value::Object(page))
    }
}

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
