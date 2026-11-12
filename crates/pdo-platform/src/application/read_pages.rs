//! Read the page mounts: the list, one mount, one file beneath a mount. Each
//! read is the Opération `page_mount` × `read` on the mount, in its Projet:
//! the list holds only the mounts the reader may read, and a refused mount is
//! not served.

use std::path::Path;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::ports::{PageFile, PageFiles, PageMountRepository, StorageError};
use crate::domain::page_mount::{requested_components, PageMount};

/// Why a mount was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadPageError {
    /// The reader may not read it.
    Refused(Denied),
    Storage(StorageError),
}

impl From<StorageError> for ReadPageError {
    fn from(error: StorageError) -> Self {
        ReadPageError::Storage(error)
    }
}

pub struct ReadPages {
    pages: Arc<dyn PageMountRepository>,
    files: Arc<dyn PageFiles>,
    host: Arc<dyn ExtensionHost>,
}

impl ReadPages {
    pub fn new(
        pages: Arc<dyn PageMountRepository>,
        files: Arc<dyn PageFiles>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self { pages, files, host }
    }

    /// The mounts `actor` may read, by name.
    pub async fn list(&self, actor: &Actor) -> Result<Vec<PageMount>, StorageError> {
        let mut mounts = self.pages.list().await?;
        mounts.retain(|mount| self.judge(actor, mount).is_ok());
        Ok(mounts)
    }

    /// The mount `name`, for `actor`: `Ok(None)` for an unknown one.
    pub async fn get(&self, actor: &Actor, name: &str) -> Result<Option<PageMount>, ReadPageError> {
        let Some(mount) = self.pages.get(name).await? else {
            return Ok(None);
        };
        self.judge(actor, &mount).map_err(ReadPageError::Refused)?;
        Ok(Some(mount))
    }

    /// The file `path` names under the mount `name`, for `actor`. `Ok(None)`
    /// for an unknown mount, a path leaving the mount, or no such file — all
    /// a 404.
    pub async fn file(
        &self,
        actor: &Actor,
        name: &str,
        path: &str,
    ) -> Result<Option<PageFile>, ReadPageError> {
        let Some(mount) = self.get(actor, name).await? else {
            return Ok(None);
        };
        let Some(components) = requested_components(path) else {
            return Ok(None);
        };
        Ok(self
            .files
            .read(Path::new(&mount.directory), &components)
            .await?)
    }

    /// Whether `actor` may read `mount`: `page_mount` × `read` on it, in its
    /// Projet.
    fn judge(&self, actor: &Actor, mount: &PageMount) -> Result<(), Denied> {
        let read = Operation::new(ResourceKind::PageMount, Verb::Read)
            .on(mount.name.as_str())
            .in_projects(mount.projects());
        authorize_read(self.host.as_ref(), actor, &read)
    }
}
