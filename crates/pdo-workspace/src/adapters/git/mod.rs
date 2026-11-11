//! Git, for the whole workspace context: the only module of this crate that
//! runs `git`. [`GitCli`] serves the [`Git`] port; the free functions below it
//! serve the contexts that have not moved onto the port yet (the Run), and the
//! adapters of the skill context (the shallow clone of a Source, the index and
//! `info/exclude` of the worktree a skill is delivered into).
//!
//! Tested against real temporary repositories (`tests/git_*.rs`).

pub mod branches;
pub mod clone;
pub mod fast_forward;
pub mod fetch;
pub mod manifest;
pub mod repository;
pub mod revisions;
pub mod scaffold;
pub mod worktrees;

use std::path::{Path, PathBuf};

use pdo_kernel::extension_host::GitCredentials;

use crate::application::ports::{BoxFuture, Git};
use crate::domain::branches::{BranchList, FetchError};
use crate::domain::clone::CloneFailure;
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

    fn fetch<'a>(
        &'a self,
        repo: &'a Path,
        credentials: &'a GitCredentials,
    ) -> BoxFuture<'a, Option<FetchError>> {
        Box::pin(fetch::fetch_all_remotes(repo, &credentials.env))
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

    fn remote_url(&self, repo: &Path) -> Option<String> {
        repository::remote_url(repo)
    }

    fn create_repository(
        &self,
        parent: &str,
        name: &str,
        files: &[ScaffoldFile],
    ) -> Result<ScaffoldOutcome, String> {
        scaffold::create_repo(parent, name, files)
    }

    fn clone_repository<'a>(
        &'a self,
        url: &'a str,
        dest: &'a Path,
        credentials: &'a GitCredentials,
        on_progress: Box<dyn Fn(&str) + Send + Sync + 'a>,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) -> BoxFuture<'a, Result<(), CloneFailure>> {
        Box::pin(async move {
            clone::clone_full(url, dest, &credentials.env, on_progress.as_ref(), cancel).await
        })
    }

    fn reach<'a>(
        &'a self,
        url: &'a str,
        credentials: &'a GitCredentials,
    ) -> BoxFuture<'a, Result<(), CloneFailure>> {
        Box::pin(clone::ls_remote(url, &credentials.env))
    }
}
