//! Mount a directory as a live page (`pdo page mount`, `POST /pages`).

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::page_journal::{journal, PageMountLock, SessionClaim};
use super::ports::{InsertError, PageFiles, PageMountRepository, Runs, StorageError};
use super::record_audit::RecordAudit;
use crate::domain::audit::ActorHint;
use crate::domain::page_mount::{check_mountable, validate_name, MountError, PageMount};

pub struct MountPageCommand {
    pub name: String,
    pub directory: PathBuf,
    /// The origin the caller declared, for the audit row.
    pub hint: ActorHint,
}

/// Why the page was not mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountPageError {
    Refused(Denied),
    /// The agent's session claim does not name a live node session.
    SessionRefused(String),
    Invalid(MountError),
    Storage(StorageError),
    /// The Run's event log refused the journal line; the mount was rolled back.
    Journal(String),
}

pub struct MountPage {
    pages: Arc<dyn PageMountRepository>,
    files: Arc<dyn PageFiles>,
    runs: Arc<dyn Runs>,
    audit: RecordAudit,
    host: Arc<dyn ExtensionHost>,
    lock: PageMountLock,
}

impl MountPage {
    pub fn new(
        pages: Arc<dyn PageMountRepository>,
        files: Arc<dyn PageFiles>,
        runs: Arc<dyn Runs>,
        audit: RecordAudit,
        host: Arc<dyn ExtensionHost>,
        lock: PageMountLock,
    ) -> Self {
        Self {
            pages,
            files,
            runs,
            audit,
            host,
            lock,
        }
    }

    /// Mount for `actor`. An agent's mount belongs to its Run (archived with
    /// it) and is journaled in the Run's log; anyone else's is audited.
    pub async fn execute(
        &self,
        actor: &Actor,
        command: MountPageCommand,
    ) -> Result<PageMount, MountPageError> {
        let operation = Operation::new(ResourceKind::PageMount, Verb::Create);
        authorize(self.host.as_ref(), actor, &operation).map_err(MountPageError::Refused)?;
        let _guard = self.lock.lock().await;
        let session = SessionClaim::of(actor);
        if let Some(claim) = &session {
            self.runs
                .verify(&claim.run_id, &claim.node_id)
                .await
                .map_err(MountPageError::SessionRefused)?;
        }

        validate_name(&command.name).map_err(MountPageError::Invalid)?;
        let directory = self
            .files
            .canonical_directory(&command.directory)
            .map_err(MountPageError::Invalid)?;
        check_mountable(&directory).map_err(MountPageError::Invalid)?;
        let mount = PageMount {
            name: command.name.clone(),
            directory: directory.to_string_lossy().into_owned(),
            run_id: session.as_ref().map(|claim| claim.run_id.clone()),
            created_at: pdo_kernel::clock::now_iso(),
        };
        match self.pages.insert(&mount).await {
            Ok(()) => {}
            Err(InsertError::Duplicate) => {
                return Err(MountPageError::Invalid(MountError::Duplicate(command.name)))
            }
            Err(InsertError::Storage(e)) => return Err(MountPageError::Storage(e)),
        }

        if let Err(error) = journal(
            &self.runs,
            &self.audit,
            session.as_ref(),
            command.hint,
            "page_mount.mounted",
            None,
            Some(serde_json::json!(mount)),
        )
        .await
        {
            if let Err(rollback) = self.pages.remove(&mount.name).await {
                tracing::error!(
                    "failed to roll back page mount {} after event error: {rollback}",
                    mount.name
                );
            }
            return Err(MountPageError::Journal(error));
        }
        Ok(mount)
    }
}
