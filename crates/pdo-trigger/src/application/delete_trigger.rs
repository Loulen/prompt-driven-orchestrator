//! Delete a Trigger (`DELETE /triggers/{id}`): `trigger` × `delete`. Its fire
//! history stays, so the stats keep counting its fires.

use pdo_kernel::{Actor, Verb};
use pdo_platform::domain::audit::ActorHint;
use tracing::error;

use super::{operation, TriggerError};
use crate::Triggers;

impl Triggers {
    pub async fn delete(
        &self,
        actor: &Actor,
        hint: ActorHint,
        id: &str,
    ) -> Result<(), TriggerError> {
        let delete = self.on_trigger(Verb::Delete, id).await;
        self.authorize(actor, &delete)?;
        // Read the "before" snapshot HERE, before the row is gone: `delete` returns
        // `bool`, never the row, so a later read would leave the audit a dead UUID.
        let before = self.store.get(id).await.ok().flatten();
        match self.store.delete(id).await {
            Ok(true) => {
                // Read in the Projets it was in: gone, it is in none.
                let read = operation(Verb::Read, Some(id)).in_projects(delete.projects.clone());
                self.announce_about(
                    serde_json::json!({
                        "type": "trigger_deleted",
                        "trigger_id": id,
                    }),
                    &read,
                );
                self.journal(
                    hint,
                    "trigger.deleted",
                    ("trigger", Some(id)),
                    before.as_ref().and_then(|t| serde_json::to_value(t).ok()),
                    None,
                )
                .await;
                Ok(())
            }
            Ok(false) => Err(TriggerError::NotFound("trigger not found".to_string())),
            Err(e) => {
                error!("failed to delete trigger: {e}");
                Err(TriggerError::Storage(
                    "failed to delete trigger".to_string(),
                ))
            }
        }
    }
}
