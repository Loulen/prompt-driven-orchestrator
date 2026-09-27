//! Remove the page mounts of a Run being archived. Called by the `run` context,
//! which holds [`PageMountLock`] for the whole archive so no agent of that Run
//! mounts a page meanwhile. A consequence of the archive, not an Operation of
//! its own.

use std::sync::Arc;

use tokio::sync::MutexGuard;

use super::page_journal::PageMountLock;
use super::ports::{PageMountRepository, StorageError};

pub struct ReleaseRunPages {
    pages: Arc<dyn PageMountRepository>,
    lock: PageMountLock,
}

impl ReleaseRunPages {
    pub fn new(pages: Arc<dyn PageMountRepository>, lock: PageMountLock) -> Self {
        Self { pages, lock }
    }

    /// Hold every page-mount mutation off until the guard drops.
    pub async fn hold(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().await
    }

    /// Remove the mounts `run_id` made; how many went.
    pub async fn execute(&self, run_id: &str) -> Result<u64, StorageError> {
        self.pages.remove_for_run(run_id).await
    }
}
