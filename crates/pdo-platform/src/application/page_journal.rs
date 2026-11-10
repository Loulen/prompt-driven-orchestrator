//! Where a page-mount mutation is journaled: in the Run's event log when an
//! agent made it from its node session, in the audit journal otherwise.

use std::sync::Arc;

use tokio::sync::{Mutex, MutexGuard};

use super::ports::Runs;
use super::record_audit::RecordAudit;
use crate::domain::audit::{ActorHint, NewAuditEntry};

/// The node session a mutation comes from, when an agent made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionClaim {
    pub run_id: String,
    pub node_id: String,
}

impl SessionClaim {
    /// The session an Actor speaks from: an agent's, or none.
    pub fn of(actor: &pdo_kernel::Actor) -> Option<Self> {
        match actor {
            pdo_kernel::Actor::Agent { run_id, node_id } => Some(Self {
                run_id: run_id.clone(),
                node_id: node_id.clone(),
            }),
            _ => None,
        }
    }
}

/// Serialises every page-mount mutation with the archive of a Run, which removes
/// that Run's mounts: a mount is never created for a Run being archived.
#[derive(Clone, Default)]
pub struct PageMountLock(Arc<Mutex<()>>);

impl PageMountLock {
    pub async fn lock(&self) -> MutexGuard<'_, ()> {
        self.0.lock().await
    }

    /// The same lock, held by a guard that owns its share of it: what a caller
    /// in another context keeps across its own awaits.
    pub async fn lock_owned(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.0.clone().lock_owned().await
    }
}

/// Journal a mount (`before` absent) or an unmount (`after` absent).
///
/// From a session: a `CommandIssued` event in the Run's log, which must land —
/// an error here makes the caller roll the mutation back. Otherwise: an audit
/// row, best-effort.
pub(crate) async fn journal(
    runs: &Arc<dyn Runs>,
    audit: &RecordAudit,
    session: Option<&SessionClaim>,
    hint: ActorHint,
    action: &str,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) -> Result<(), String> {
    let mount = before.as_ref().or(after.as_ref()).cloned();
    let name = mount
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(claim) = session {
        return runs
            .record_command(
                &claim.run_id,
                &claim.node_id,
                action,
                mount.unwrap_or(serde_json::Value::Null),
            )
            .await;
    }
    audit
        .execute(NewAuditEntry {
            actor_hint: hint.as_hint().to_string(),
            action: action.to_string(),
            target_kind: Some("page_mount".to_string()),
            target_id: Some(name),
            before,
            after,
        })
        .await;
    Ok(())
}
