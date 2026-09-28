//! The port of the trigger context that what is still legacy implements: the
//! instance settings it reads and the pause it writes (the settings context).

use std::sync::Arc;
use std::time::Duration;

use pdo_trigger::application::ports::{BoxFuture, TriggerSettings};

use crate::instance_config;

/// The trigger context's resources: the instance settings it reads (the pause,
/// the guard timeout), still legacy, and the daemon's fault-injection knob.
#[derive(Clone)]
pub struct TriggerResources {
    pub settings: Arc<dyn TriggerSettings>,
    pub panic_on_trigger_name: Option<String>,
}

/// The pause and the guard timeout of `instance_config`, read fresh at every call.
pub(crate) struct LegacyTriggerSettings {
    pub(crate) db: sqlx::SqlitePool,
}

impl TriggerSettings for LegacyTriggerSettings {
    fn paused(&self) -> BoxFuture<'_, Result<bool, String>> {
        Box::pin(async move {
            instance_config::triggers_paused(&self.db)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn set_paused(&self, paused: bool) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            instance_config::set_triggers_paused(&self.db, paused)
                .await
                .map_err(|e| e.to_string())
        })
    }

    /// `stored → env → default`. Stored is seconds; the env seam is ms.
    fn guard_timeout(&self) -> BoxFuture<'_, Duration> {
        Box::pin(async move {
            let stored_secs = instance_config::get(&self.db)
                .await
                .ok()
                .and_then(|c| c.guard_timeout_secs)
                .map(|n| n as u64);
            pdo_trigger::adapters::env::guard_timeout_with(stored_secs)
        })
    }
}
