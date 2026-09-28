//! Uncombine (`DELETE /stats/absorptions/member`): take one member out of its
//! absorption. Removing the last member leaves no row for that absorbent — the
//! absorption is gone, and its rows come back. Taking a Pipeline out also takes
//! its Nodes out of the Node absorptions of its former absorbent's scope
//! (#892, grilling Q13).

use std::collections::BTreeSet;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::domain::audit::NewAuditEntry;

use super::AbsorptionWrites;
use crate::application::ports::{AbsorptionStore, AuditJournal, RunEvents};
use crate::domain::absorption::list::{group, AbsorptionList};
use crate::domain::absorption::request::unsupported_dimension;
use crate::domain::absorption::scopes::{released_nodes, remaining_pipelines};
use crate::domain::absorption::{AbsorptionRow, NODE, PIPELINE};

/// The member to take out: its dimension, its scope, its key.
#[derive(Debug, Clone)]
pub struct UncombineCommand {
    pub dimension: String,
    pub scope: String,
    pub member: String,
}

/// Why the Uncombine did not happen.
#[derive(Debug)]
pub enum UncombineError {
    Refused(Denied),
    Invalid(String),
    /// The key was not absorbed.
    NotCombined,
    Failed(String),
}

pub struct UncombineMember {
    store: Arc<dyn AbsorptionStore>,
    events: Arc<dyn RunEvents>,
    audit: Arc<dyn AuditJournal>,
    host: Arc<dyn ExtensionHost>,
    writes: AbsorptionWrites,
}

impl UncombineMember {
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

    pub async fn execute(
        &self,
        actor: &Actor,
        hint: &str,
        command: UncombineCommand,
    ) -> Result<AbsorptionList, UncombineError> {
        let operation =
            Operation::new(ResourceKind::StatsAbsorption, Verb::Delete).on(command.member.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(UncombineError::Refused)?;
        if let Some(message) = unsupported_dimension(&command.dimension) {
            return Err(UncombineError::Invalid(message));
        }
        let failed =
            |e: anyhow::Error| UncombineError::Failed(format!("stats absorption failed: {e}"));
        let _write = self.writes.lock().await;
        let rows = self.store.list().await.map_err(failed)?;
        let Some(absorbent) = rows
            .iter()
            .find(|row| {
                row.dimension == command.dimension
                    && row.scope == command.scope
                    && row.member == command.member
            })
            .map(|row| row.absorbent.clone())
        else {
            return Err(UncombineError::NotCombined);
        };
        let released = if command.dimension == PIPELINE {
            self.released(&rows, &command.member, &absorbent)
                .await
                .map_err(failed)?
        } else {
            Vec::new()
        };
        self.store
            .remove(
                &command.dimension,
                &command.scope,
                &command.member,
                &absorbent,
                &released,
            )
            .await
            .map_err(failed)?;
        self.audit
            .record(NewAuditEntry {
                actor_hint: hint.to_string(),
                action: "stats_absorption.uncombine".to_string(),
                target_kind: Some("stats_absorption".to_string()),
                target_id: Some(command.member.clone()),
                before: Some(serde_json::json!({
                    "dimension": command.dimension,
                    "scope": command.scope,
                    "member": command.member,
                })),
                after: None,
            })
            .await;
        Ok(group(self.store.list().await.map_err(failed)?))
    }

    /// The Node members that leave `absorbent`'s Node absorptions with
    /// Pipeline `removed`. Which Pipeline a Node ran under is read off the
    /// event log.
    async fn released(
        &self,
        rows: &[AbsorptionRow],
        removed: &str,
        absorbent: &str,
    ) -> anyhow::Result<Vec<String>> {
        if !rows
            .iter()
            .any(|row| row.dimension == NODE && row.scope == absorbent)
        {
            return Ok(Vec::new());
        }
        let gone = self.events.nodes_started_under(removed).await?;
        let mut staying: BTreeSet<String> = self.events.nodes_started_under(absorbent).await?;
        for pipeline in remaining_pipelines(rows, removed, absorbent) {
            staying.extend(self.events.nodes_started_under(&pipeline).await?);
        }
        Ok(released_nodes(rows, absorbent, &gone, &staying))
    }
}
