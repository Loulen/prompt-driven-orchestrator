//! What the workspace needs from the outside world. The composition root plugs
//! an adapter into each port; the use cases only ever see the traits.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::domain::branches::{BranchList, FetchError};
use crate::domain::browse::{BrowseEntry, BrowseRootError};
use crate::domain::fast_forward::{FastForwardDone, FastForwardRefused};
use crate::domain::project::{Project, StoredSetting};
use crate::domain::provisioning::{
    ProvisioningPlan, ProvisioningRules, ProvisioningScope, ScopedRules,
};
use crate::domain::scaffold::{ScaffoldFile, ScaffoldOutcome};

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A store that could not answer. The message is for the log, never the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageError(pub String);

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type StoreResult<T> = Result<T, StorageError>;

/// Where the Projets live.
pub trait ProjectRepository: Send + Sync {
    /// Every Projet, newest first.
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Project>>>;
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Project>>>;
    /// The Projet that owns `path`, compared verbatim (ADR-0033).
    fn owner_of<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Option<Project>>>;
    /// Materialise a Projet from a bare name, with a fresh id.
    fn create<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Project>>;
    fn rename<'a>(&'a self, id: &'a str, name: &'a str) -> BoxFuture<'a, StoreResult<()>>;
    fn set_harness<'a>(
        &'a self,
        id: &'a str,
        harness: Option<&'a str>,
    ) -> BoxFuture<'a, StoreResult<()>>;
    fn set_auto_fail<'a>(
        &'a self,
        id: &'a str,
        auto_fail: Option<bool>,
    ) -> BoxFuture<'a, StoreResult<()>>;
    fn set_agent_choice<'a>(
        &'a self,
        id: &'a str,
        choice: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>>;
    fn set_skills<'a>(
        &'a self,
        id: &'a str,
        skills: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>>;
    /// Attach a free path. The caller has already checked it is free.
    fn add_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>>;
    /// Detach `path` from whichever Projet holds it.
    fn remove_member<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>>;
    /// Delete the Projet and free its members. `false` when there was none.
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>>;
}

/// Where the provisioning rules of each scope live.
pub trait ProvisioningRulesRepository: Send + Sync {
    /// The stored rules, empty when none are stored.
    fn load<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
    ) -> BoxFuture<'a, StoreResult<ProvisioningRules>>;
    fn save<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
        rules: &'a ProvisioningRules,
    ) -> BoxFuture<'a, StoreResult<()>>;
}

/// A repository a Run worked in, with the last Run that started there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRepository {
    pub path: String,
    pub run_id: String,
}

impl RecentRepository {
    pub fn new(path: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            run_id: run_id.into(),
        }
    }
}

/// The repositories Runs worked in, most recent first, each once with its
/// last Run. Uncapped: the caller keeps what the actor may read (CORE-5),
/// then caps.
pub trait RecentRepositories: Send + Sync {
    fn recent(&self) -> BoxFuture<'_, StoreResult<Vec<RecentRepository>>>;
}

/// Who hears that a Projet changed (the live pipeline stream, the names the
/// extensions read). Awaited once the change is stored, before the gesture
/// answers: whoever hears it next reads the Projet as it now is.
pub trait ProjectEvents: Send + Sync {
    fn project_changed<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, ()>;
}

/// Who forgets what pointed at a Projet once it is deleted (the validations of
/// the pipelines). Awaited once the deletion is stored, before the Projet's
/// change is announced.
pub trait ProjectDeletions: Send + Sync {
    fn project_deleted<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, ()>;
}

/// Git. One adapter implements it, tested against real temporary repositories.
pub trait Git: Send + Sync {
    /// `path` as a repository a Run can work in: absolute, a directory, a repo.
    fn validate_repository(&self, path: &str) -> Result<PathBuf, String>;
    /// The branches a Run can be cut from, read locally.
    fn branches(&self, repo: &Path) -> Result<BranchList, String>;
    /// Fetch every remote; `None` on success, else why it did not happen.
    fn fetch<'a>(&'a self, repo: &'a Path) -> BoxFuture<'a, Option<FetchError>>;
    /// Whether `branch` is a local branch of `repo`.
    fn is_local_branch(&self, repo: &Path, branch: &str) -> bool;
    fn fast_forward(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<FastForwardDone, FastForwardRefused>;
    /// Create (or reuse) `<parent>/<name>` as a repository holding `files`.
    fn create_repository(
        &self,
        parent: &str,
        name: &str,
        files: &[ScaffoldFile],
    ) -> Result<ScaffoldOutcome, String>;
}

/// The source tree provisioning reads: its files and what git provides.
pub trait SourceTree: Send + Sync {
    fn plan(
        &self,
        repository: &Path,
        scoped: &[ScopedRules],
        git_ref: &str,
    ) -> anyhow::Result<ProvisioningPlan>;
}

/// One level of a directory, as the explorer lists it.
#[derive(Debug)]
pub struct Listing {
    /// The directory listed, canonicalized.
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<BrowseEntry>,
    pub truncated: bool,
    /// Set when the directory could be named but not listed (EACCES…).
    pub error: Option<String>,
}

/// The filesystem, read one level at a time.
pub trait FileBrowser: Send + Sync {
    fn browse(
        &self,
        path: Option<&str>,
        files: bool,
        hidden: bool,
    ) -> Result<Listing, BrowseRootError>;
}
