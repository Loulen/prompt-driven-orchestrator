//! The Apparence of the caller (CONTEXT.md → *Apparence*): read and replaced
//! for the Actor the daemon established for the request, never another's.
//!
//! No Operation is asked: an Actor's own presentation choices are no
//! Ressource of the instance, and no route reaches another Actor's. Without an
//! identity extension, every call comes from the Opérateur local: one document
//! for the instance. Only a person writes one (the Opérateur local, an Actor an
//! extension identified): an agent's node or a Trigger has no interface, and
//! would leave a row per NodeRun behind.

use pdo_kernel::Actor;
use tracing::error;

use crate::application::SettingsError;
use crate::domain::appearance::{AppearanceDocument, AppearanceError};
use crate::Settings;

impl From<AppearanceError> for SettingsError {
    fn from(error: AppearanceError) -> Self {
        match error {
            AppearanceError::TooLarge(_) => SettingsError::TooLarge(error.to_string()),
            AppearanceError::Malformed(_) => SettingsError::Invalid(error.to_string()),
        }
    }
}

impl Settings {
    /// `actor`'s Apparence; `None` while it never wrote one.
    pub async fn appearance(
        &self,
        actor: &Actor,
    ) -> Result<Option<AppearanceDocument>, SettingsError> {
        self.appearances.get(actor).await.map_err(|e| {
            error!("failed to read the appearance of a {}: {e}", actor.kind());
            SettingsError::Storage(e.0)
        })
    }

    /// Replace `actor`'s Apparence with the document `raw` holds, once its
    /// shape is checked; answers the document kept.
    pub async fn set_appearance(
        &self,
        actor: &Actor,
        raw: &[u8],
    ) -> Result<AppearanceDocument, SettingsError> {
        if !matches!(actor, Actor::LocalOperator | Actor::Identified { .. }) {
            return Err(SettingsError::Invalid(format!(
                "a {} has no appearance: only a person keeps one",
                actor.kind()
            )));
        }
        let document = AppearanceDocument::parse(raw)?;
        self.appearances.put(actor, &document).await.map_err(|e| {
            error!("failed to write the appearance of a {}: {e}", actor.kind());
            SettingsError::Storage(e.0)
        })?;
        Ok(document)
    }
}
