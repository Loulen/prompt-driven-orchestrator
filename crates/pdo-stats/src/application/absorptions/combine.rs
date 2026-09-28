//! Combine (`POST /stats/absorptions`): create or extend an absorption. Asks
//! the Autorisation d'opération before anything is read or written, then
//! refuses a request that does not fit (ADR-0077, ADR-0078), writes, audits,
//! and answers the whole list as it stands after the write.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::domain::audit::NewAuditEntry;

use super::{histories, AbsorptionWrites};
use crate::application::ports::{AbsorptionStore, AuditJournal, RunEvents};
use crate::domain::absorption::list::{group, AbsorptionList};
use crate::domain::absorption::request::{
    absorbent_and_members, frozen_scope, invalid_scope, unsupported_dimension, CombineRequest,
};
use crate::domain::absorption::scopes::{couple_cover, covered_couples, effort_cover};
use crate::domain::absorption::{Scope, EFFORT, ORIGIN_MANUAL};

/// Why the Combine did not happen.
#[derive(Debug)]
pub enum CombineError {
    Refused(Denied),
    /// The request does not fit: `400` with the message.
    Invalid(String),
    /// The store or the event log failed.
    Failed(String),
}

pub struct CombineAbsorption {
    store: Arc<dyn AbsorptionStore>,
    events: Arc<dyn RunEvents>,
    audit: Arc<dyn AuditJournal>,
    host: Arc<dyn ExtensionHost>,
    writes: AbsorptionWrites,
}

impl CombineAbsorption {
    pub fn new(
        store: Arc<dyn AbsorptionStore>,
        events: Arc<dyn RunEvents>,
        audit: Arc<dyn AuditJournal>,
        host: Arc<dyn ExtensionHost>,
        writes: AbsorptionWrites,
    ) -> Self {
        Self {
            store,
            events,
            audit,
            host,
            writes,
        }
    }

    /// Combine for `actor`; `hint` is the origin the caller declared, for the
    /// audit row.
    pub async fn execute(
        &self,
        actor: &Actor,
        hint: &str,
        body: CombineRequest,
    ) -> Result<AbsorptionList, CombineError> {
        let operation = Operation::new(ResourceKind::StatsAbsorption, Verb::Create);
        authorize(self.host.as_ref(), actor, &operation).map_err(CombineError::Refused)?;
        if let Some(message) = unsupported_dimension(&body.dimension) {
            return Err(CombineError::Invalid(message));
        }
        let failed =
            |e: anyhow::Error| CombineError::Failed(format!("stats absorption failed: {e}"));
        let _write = self.writes.lock().await;
        let rows = self.store.list().await.map_err(failed)?;
        if let Some(message) = invalid_scope(&body, &rows) {
            return Err(CombineError::Invalid(message));
        }
        let (absorbent, members) = absorbent_and_members(&body).map_err(CombineError::Invalid)?;
        if frozen_scope(&body.dimension) {
            let covered = if body.dimension == EFFORT {
                effort_cover(&rows, &body.scope)
            } else {
                match couple_cover(&rows, &body.scope) {
                    Some(cover) => {
                        let histories = histories(self.events.as_ref(), &cover.pipelines)
                            .await
                            .map_err(failed)?;
                        covered_couples(&cover, &body.scope_name, &histories)
                    }
                    None => Vec::new(),
                }
            };
            self.store
                .combine_frozen(
                    &body.dimension,
                    &covered,
                    &absorbent,
                    &members,
                    ORIGIN_MANUAL,
                )
                .await
                .map_err(failed)?;
        } else {
            let scope = Scope {
                dimension: &body.dimension,
                key: &body.scope,
                name: &body.scope_name,
            };
            self.store
                .combine(scope, &absorbent, &members, ORIGIN_MANUAL)
                .await
                .map_err(failed)?;
        }
        self.audit
            .record(NewAuditEntry {
                actor_hint: hint.to_string(),
                action: "stats_absorption.combine".to_string(),
                target_kind: Some("stats_absorption".to_string()),
                target_id: Some(absorbent.key.clone()),
                before: None,
                after: Some(serde_json::json!({
                    "dimension": body.dimension,
                    "scope": body.scope,
                    "absorbent": absorbent,
                    "members": members,
                })),
            })
            .await;
        Ok(group(self.store.list().await.map_err(failed)?))
    }
}
