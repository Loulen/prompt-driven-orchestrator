//! Mount a directory as a live page (`pdo page mount`, `POST /pages`), with an
//! optional reading guide checked before anything is stored.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::page_journal::{journal, PageMountLock, SessionClaim};
use super::place_page_mount::PlacePageMount;
use super::ports::{InsertError, PageFiles, PageMountRepository, Runs, StorageError};
use super::record_audit::RecordAudit;
use crate::domain::audit::ActorHint;
use crate::domain::page_guide::{self, GuideEntry, GuideLink};
use crate::domain::page_mount::{check_mountable, validate_name, MountError, PageMount};

pub struct MountPageCommand {
    pub name: String,
    pub directory: PathBuf,
    /// The guide file's text, as written (YAML or JSON), if the page has one.
    pub guide: Option<String>,
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
    place: Arc<PlacePageMount>,
}

impl MountPage {
    pub fn new(
        pages: Arc<dyn PageMountRepository>,
        files: Arc<dyn PageFiles>,
        runs: Arc<dyn Runs>,
        audit: RecordAudit,
        host: Arc<dyn ExtensionHost>,
        lock: PageMountLock,
        place: Arc<PlacePageMount>,
    ) -> Self {
        Self {
            pages,
            files,
            runs,
            audit,
            host,
            lock,
            place,
        }
    }

    /// Mount for `actor`. An agent's mount belongs to its Run (archived with
    /// it), is judged in the Projet of that Run and journaled in its log;
    /// anyone else's is judged in no Projet and audited.
    pub async fn execute(
        &self,
        actor: &Actor,
        command: MountPageCommand,
    ) -> Result<PageMount, MountPageError> {
        let operation = Operation::new(ResourceKind::PageMount, Verb::Create);
        let operation = self.place.execute(actor, operation).await;
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
        let guide = match &command.guide {
            Some(raw) => Some(
                self.checked_guide(raw, &directory)
                    .await
                    .map_err(|why| MountPageError::Invalid(MountError::Invalid(why)))?,
            ),
            None => None,
        };
        let mount = PageMount {
            name: command.name.clone(),
            directory: directory.to_string_lossy().into_owned(),
            run_id: session.as_ref().map(|claim| claim.run_id.clone()),
            node_id: session.as_ref().map(|claim| claim.node_id.clone()),
            created_at: pdo_kernel::clock::now_iso(),
            guide,
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

    /// The guide's entries, once its format is right and every link leads
    /// somewhere: a Run and node that exist, a file beneath the mount.
    async fn checked_guide(
        &self,
        raw: &str,
        directory: &std::path::Path,
    ) -> Result<Vec<GuideEntry>, String> {
        let entries = page_guide::parse(raw)?;
        for (index, (entry, link)) in entries.iter().enumerate() {
            let at = |why: String| format!("guide entry {}: {why}", index + 1);
            let (run_id, node_id) = match link {
                GuideLink::Run { run_id } | GuideLink::Review { run_id } => (run_id, None),
                GuideLink::Node { run_id, node_id }
                | GuideLink::Artifact {
                    run_id, node_id, ..
                } => (run_id, Some(node_id)),
                GuideLink::Path(components) => {
                    if !self.files.exists(directory, components).await {
                        return Err(at(format!("`{}` is not a file of the page", entry.link)));
                    }
                    continue;
                }
                GuideLink::Extension | GuideLink::Anchor => continue,
            };
            let Some(nodes) = self.runs.node_ids(run_id).await else {
                return Err(at(format!("Run `{run_id}` not found")));
            };
            if let Some(node_id) = node_id {
                if !nodes.iter().any(|n| n == node_id) {
                    return Err(at(format!("node `{node_id}` not found in Run `{run_id}`")));
                }
            }
        }
        Ok(entries.into_iter().map(|(entry, _)| entry).collect())
    }
}
