//! Unmount a page (`pdo page unmount`, `DELETE /pages/{name}`).

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::page_journal::{journal, PageMountLock, SessionClaim};
use super::ports::{PageMountRepository, Runs, StorageError};
use super::record_audit::RecordAudit;
use crate::domain::audit::ActorHint;
use crate::domain::page_mount::PageMount;

/// Why the page was not unmounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnmountPageError {
    Refused(Denied),
    SessionRefused(String),
    NotFound(String),
    Storage(StorageError),
    /// The Run's event log refused the journal line; the mount was restored.
    Journal(String),
}

pub struct UnmountPage {
    pages: Arc<dyn PageMountRepository>,
    runs: Arc<dyn Runs>,
    audit: RecordAudit,
    host: Arc<dyn ExtensionHost>,
    lock: PageMountLock,
}

impl UnmountPage {
    pub fn new(
        pages: Arc<dyn PageMountRepository>,
        runs: Arc<dyn Runs>,
        audit: RecordAudit,
        host: Arc<dyn ExtensionHost>,
        lock: PageMountLock,
    ) -> Self {
        Self {
            pages,
            runs,
            audit,
            host,
            lock,
        }
    }

    pub async fn execute(
        &self,
        actor: &Actor,
        name: &str,
        hint: ActorHint,
    ) -> Result<PageMount, UnmountPageError> {
        let operation = Operation::new(ResourceKind::PageMount, Verb::Delete).on(name);
        authorize(self.host.as_ref(), actor, &operation).map_err(UnmountPageError::Refused)?;
        let _guard = self.lock.lock().await;
        let session = SessionClaim::of(actor);
        if let Some(claim) = &session {
            self.runs
                .verify(&claim.run_id, &claim.node_id)
                .await
                .map_err(UnmountPageError::SessionRefused)?;
        }

        let mount = match self.pages.remove(name).await {
            Ok(Some(mount)) => mount,
            Ok(None) => return Err(UnmountPageError::NotFound(name.to_string())),
            Err(e) => return Err(UnmountPageError::Storage(e)),
        };

        if let Err(error) = journal(
            &self.runs,
            &self.audit,
            session.as_ref(),
            hint,
            "page_mount.unmounted",
            Some(serde_json::json!(mount)),
            None,
        )
        .await
        {
            if let Err(restore) = self.pages.insert(&mount).await {
                tracing::error!(
                    "failed to restore page mount {} after event error: {restore}",
                    mount.name
                );
            }
            return Err(UnmountPageError::Journal(error));
        }
        Ok(mount)
    }
}
