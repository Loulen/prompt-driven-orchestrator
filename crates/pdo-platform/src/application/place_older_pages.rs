//! The mounts older than their owner and their Projet take them from their
//! Run, once, at boot: the Projet the Run was launched in, and the one who
//! created it. A mount outside any Run, or whose Run names neither, stays in
//! no Projet, without an owner.

use std::sync::Arc;

use super::ports::{PageMountRepository, Runs, StorageError};

pub struct PlaceOlderPages {
    pages: Arc<dyn PageMountRepository>,
    runs: Arc<dyn Runs>,
}

impl PlaceOlderPages {
    pub fn new(pages: Arc<dyn PageMountRepository>, runs: Arc<dyn Runs>) -> Self {
        Self { pages, runs }
    }

    /// Place every mount of a Run that has neither an owner nor a Projet;
    /// how many were placed.
    pub async fn execute(&self) -> Result<usize, StorageError> {
        let mut placed = 0;
        for mount in self.pages.list().await? {
            let Some(run_id) = &mount.run_id else {
                continue;
            };
            if mount.owner.is_some() || mount.project_id.is_some() {
                continue;
            }
            let project = self.runs.projects_of_run(run_id).await.into_iter().next();
            let owner = self.runs.creator_of_run(run_id).await;
            if project.is_none() && owner.is_none() {
                continue;
            }
            self.pages
                .place(&mount.name, owner.as_ref(), project.as_deref())
                .await?;
            placed += 1;
        }
        Ok(placed)
    }
}
