//! Read the page mounts: the list, one mount, one file beneath a mount. Reads
//! are no Operation: they go through no authorization.

use std::path::Path;
use std::sync::Arc;

use super::ports::{PageFile, PageFiles, PageMountRepository, StorageError};
use crate::domain::page_mount::{requested_components, PageMount};

pub struct ReadPages {
    pages: Arc<dyn PageMountRepository>,
    files: Arc<dyn PageFiles>,
}

impl ReadPages {
    pub fn new(pages: Arc<dyn PageMountRepository>, files: Arc<dyn PageFiles>) -> Self {
        Self { pages, files }
    }

    pub async fn list(&self) -> Result<Vec<PageMount>, StorageError> {
        self.pages.list().await
    }

    pub async fn get(&self, name: &str) -> Result<Option<PageMount>, StorageError> {
        self.pages.get(name).await
    }

    /// The file `path` names under the mount `name`. `Ok(None)` for an unknown
    /// mount, a path leaving the mount, or no such file — all a 404.
    pub async fn file(&self, name: &str, path: &str) -> Result<Option<PageFile>, StorageError> {
        let Some(mount) = self.pages.get(name).await? else {
            return Ok(None);
        };
        let Some(components) = requested_components(path) else {
            return Ok(None);
        };
        self.files
            .read(Path::new(&mount.directory), &components)
            .await
    }
}
