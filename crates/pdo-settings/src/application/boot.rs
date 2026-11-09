//! What the daemon does with the settings at boot, on its own: say once that a
//! retired setting is still stored, and warm the harnesses' catalogues. Both
//! are transitions of the daemon, never an Operation (D10).

use pdo_sandbox::adapters::env::default_sandbox_with;
use pdo_sandbox::domain::retired_settings::retired_settings_warning;
use tracing::warn;

use super::ports::InstanceConfigStore;
use crate::Settings;

/// The ONE boot line owed to a user whose instance still stores `image_source`
/// or `dockerfile_path`: those values are now **inert**, and a value that stops
/// mattering must never do so in silence. Run it right after the schema is up
/// and before anything consumes a setting — the only moment where "one line
/// per boot" is a fact rather than a hope. Answers the line it logged.
///
/// Best-effort: a failed read here must not stop a daemon from starting over a
/// message about a setting that is already inert.
pub async fn warn_about_retired_settings(config: &dyn InstanceConfigStore) -> Option<String> {
    let stored = match config.retired_sandbox_values().await {
        Ok(stored) => stored,
        Err(e) => {
            warn!("#471: cannot check for retired sandbox image settings: {e}");
            return None;
        }
    };
    // The profile a Run currently falls back to: the concrete thing to edit.
    let hint = config.get().await.ok().and_then(|cfg| {
        default_sandbox_with(cfg.default_sandbox)
            .profile()
            .map(String::from)
    });
    let message = retired_settings_warning(&stored, hint.as_deref())?;
    warn!("{message}");
    Some(message)
}

impl Settings {
    /// Probe every installed harness's catalogue once, so the first page
    /// fetch answers warm. A page opened before the probes answered reads the
    /// settings anew once they have (CORE-232: Diagnostics kept « probe
    /// failed » for a harness the warm-up had since loaded).
    pub async fn warm_harness_catalogues(&self) {
        self.harnesses.warm().await;
        self.catalogue_events.settings_changed();
    }
}
