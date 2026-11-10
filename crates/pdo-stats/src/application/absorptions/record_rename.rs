//! The absorption a library rename leaves behind (ADR-0077 §4, #891): when a
//! rename changes a Pipeline's id, the new key carries the old key's series,
//! as a Combine would, origin `rename`. Flattened like any Combine — a chain
//! A → B → C ends as C absorbing A and B. When the old key was itself
//! absorbed, the new key joins that same absorbent instead: the operator's
//! reading is kept, and there is still only one level.
//!
//! The rename itself was the gesture (the pipeline context asked the
//! Autorisation d'opération for it); this is what it entails, so it asks
//! nothing more.

use std::sync::Arc;

use super::AbsorptionWrites;
use crate::application::ports::AbsorptionStore;
use crate::domain::absorption::{Named, Scope, ORIGIN_RENAME, PIPELINE};

pub struct RecordRename {
    store: Arc<dyn AbsorptionStore>,
    writes: AbsorptionWrites,
}

impl RecordRename {
    pub fn new(store: Arc<dyn AbsorptionStore>, writes: AbsorptionWrites) -> Self {
        Self { store, writes }
    }

    pub async fn execute(&self, old: &Named, new: &Named) -> anyhow::Result<()> {
        if old.key == new.key {
            return Ok(());
        }
        let _write = self.writes.lock().await;
        let absorbent = self
            .store
            .list()
            .await?
            .into_iter()
            .find(|row| row.dimension == PIPELINE && row.scope.is_empty() && row.member == old.key)
            .map(|row| Named {
                key: row.absorbent,
                name: row.absorbent_name,
            });
        let scope = Scope {
            dimension: PIPELINE,
            key: "",
            name: "",
        };
        match absorbent {
            Some(absorbent) if absorbent.key != new.key => {
                self.store
                    .combine(scope, &absorbent, std::slice::from_ref(new), ORIGIN_RENAME)
                    .await
            }
            _ => {
                self.store
                    .combine(scope, new, std::slice::from_ref(old), ORIGIN_RENAME)
                    .await
            }
        }
    }

    /// [`Self::execute`] for the rename handlers: the files have already
    /// moved, so a failed write here is logged, never turned into a failed
    /// rename.
    pub async fn best_effort(&self, old: Named, new: Named) {
        if let Err(e) = self.execute(&old, &new).await {
            tracing::warn!(
                "stats absorption for the rename {} -> {} failed: {e}",
                old.key,
                new.key
            );
        }
    }
}
