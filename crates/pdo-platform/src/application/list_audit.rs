//! Read the audit feed (`GET /audit`): newest first, filtered.

use std::sync::Arc;

use super::ports::{AuditLog, StorageError};
use crate::domain::audit::{AuditEntry, AuditFilter};

pub struct ListAudit {
    audit: Arc<dyn AuditLog>,
}

impl ListAudit {
    pub fn new(audit: Arc<dyn AuditLog>) -> Self {
        Self { audit }
    }

    pub async fn execute(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, StorageError> {
        self.audit.list(filter).await
    }
}
