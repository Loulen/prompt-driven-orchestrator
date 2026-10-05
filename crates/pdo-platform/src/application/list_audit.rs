//! Read the audit feed (`GET /audit`): newest first, filtered, each row's
//! Actor with the display name the extensions give it.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;

use super::ports::{AuditLog, StorageError};
use crate::domain::audit::{AuditEntry, AuditFilter};

pub struct ListAudit {
    audit: Arc<dyn AuditLog>,
    extensions: Arc<dyn ExtensionHost>,
}

impl ListAudit {
    pub fn new(audit: Arc<dyn AuditLog>, extensions: Arc<dyn ExtensionHost>) -> Self {
        Self { audit, extensions }
    }

    pub async fn execute(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, StorageError> {
        let mut rows = self.audit.list(filter).await?;
        for row in &mut rows {
            row.actor_display = row
                .actor
                .as_ref()
                .and_then(|actor| self.extensions.display_name(actor));
        }
        Ok(rows)
    }
}
