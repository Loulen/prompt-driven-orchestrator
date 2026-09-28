//! Pause or resume every scheduled fire (`POST /triggers/pause`): `trigger` ×
//! `pause`, the global kill-switch.
//!
//! Persists the pause in the instance settings (a daemon-wide scheduler gate,
//! outside the `/settings` machinery) and announces it, because there is no
//! trigger polling and a second tab would otherwise never learn. Idempotent.
//!
//! The per-Trigger `enabled` flag is never touched: pause and per-Trigger disable
//! are orthogonal channels, which is what makes "restore the prior state on
//! unpause" free.

use pdo_kernel::{Actor, Verb};
use pdo_platform::domain::audit::ActorHint;

use super::{operation, TriggerError, PAUSE};
use crate::Triggers;

impl Triggers {
    pub async fn pause(
        &self,
        actor: &Actor,
        hint: ActorHint,
        paused: bool,
    ) -> Result<(), TriggerError> {
        self.authorize(actor, &operation(Verb::Action(PAUSE.to_string()), None))?;
        // The write is blind, so capture the "before" flag first or the audit entry
        // cannot show the direction of the flip.
        let before = self.settings.paused().await.ok();
        self.settings
            .set_paused(paused)
            .await
            .map_err(TriggerError::Storage)?;
        self.announce(serde_json::json!({
            "type": "triggers_paused",
            "paused": paused,
        }));
        self.journal(
            hint,
            "triggers.pause_changed",
            ("instance", None),
            Some(serde_json::json!({ "paused": before })),
            Some(serde_json::json!({ "paused": paused })),
        )
        .await;
        Ok(())
    }
}
