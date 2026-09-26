//! Git, for the whole workspace context: the only module of this crate that
//! runs `git`. [`GitCli`] serves the [`Git`] port; the free functions below it
//! serve the contexts that have not moved onto the port yet (the Run).
//!
//! Tested against real temporary repositories (`tests/git_*.rs`).

pub mod branches;
pub mod fast_forward;
pub mod fetch;
pub mod manifest;
pub mod repository;
pub mod scaffold;
pub mod worktrees;

use std::path::{Path, PathBuf};

use crate::application::ports::{BoxFuture, Git};
use crate::domain::branches::{BranchList, FetchError};
use crate::domain::fast_forward::{FastForwardDone, FastForwardRefused};
use crate::domain::scaffold::{ScaffoldFile, ScaffoldOutcome};

/// The [`Git`] port over the `git` executable.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitCli;

impl Git for GitCli {
    fn validate_repository(&self, path: &str) -> Result<PathBuf, String> {
        repository::validate_target_repo(path)
    }

    fn branches(&self, repo: &Path) -> Result<BranchList, String> {
        branches::branch_list(repo)
    }

    fn fetch<'a>(&'a self, repo: &'a Path) -> BoxFuture<'a, Option<FetchError>> {
        Box::pin(fetch::fetch_all_remotes(repo))
    }

    fn is_local_branch(&self, repo: &Path, branch: &str) -> bool {
        repository::ref_exists(repo, &format!("refs/heads/{branch}"))
    }

    fn fast_forward(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<FastForwardDone, FastForwardRefused> {
        fast_forward::fast_forward_branch(repo, branch)
    }

    fn create_repository(
        &self,
        parent: &str,
        name: &str,
        files: &[ScaffoldFile],
    ) -> Result<ScaffoldOutcome, String> {
        scaffold::create_repo(parent, name, files)
    }
}
