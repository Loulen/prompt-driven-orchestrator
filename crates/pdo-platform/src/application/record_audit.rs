//! Journal an out-of-Run configuration mutation (ADR-0044), after it committed.
//!
//! Not an Operation of its own: it is what another mutation leaves behind, so it
//! goes through no authorization. The contexts that mutate configuration outside
//! a Run (Triggers, stats absorptions, page mounts) call it.

use std::sync::Arc;

use super::ports::AuditLog;
use crate::domain::audit::NewAuditEntry;

#[derive(Clone)]
pub struct RecordAudit {
    audit: Arc<dyn AuditLog>,
}

impl RecordAudit {
    pub fn new(audit: Arc<dyn AuditLog>) -> Self {
        Self { audit }
    }

    /// Best-effort: **never** fails the calling mutation. A failed write is
    /// logged as an error (never a warning — a missing audit row is a real
    /// observability hole).
    pub async fn execute(&self, entry: NewAuditEntry) {
        let (action, target) = (entry.action.clone(), entry.target_id.clone());
        if let Err(err) = self.audit.record(entry).await {
            tracing::error!(action = %action, target_id = ?target, "audit_log: écriture échouée: {err}");
        }
    }
}
