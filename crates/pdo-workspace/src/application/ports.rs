//! What the workspace needs from the outside world. The composition root plugs
//! an adapter into each port; the use cases only ever see the traits.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::domain::branches::{BranchList, FetchError};
use crate::domain::browse::{BrowseEntry, BrowseRootError};
use crate::domain::clone::CloneFailure;
use crate::domain::fast_forward::{FastForwardDone, FastForwardRefused};
use crate::domain::project::{DeletedProject, Project, StoredSetting};
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
    /// The Projets `path` is a member of, compared verbatim (ADR-0033), in
    /// the order it joined them (ADR-0087: a path may be in several).
    fn containing<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Vec<Project>>>;
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
    /// The default primary repository of the Projet (`None` clears it).
    fn set_primary_repo<'a>(
        &'a self,
        id: &'a str,
        repo: Option<&'a str>,
    ) -> BoxFuture<'a, StoreResult<()>>;
    /// Attach a path that is not yet a member of this Projet.
    fn add_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>>;
    /// Detach `path` from the Projet `id`, and from no other.
    fn remove_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>>;
    /// Delete the Projet and free its members, keeping its id and its name
    /// among the deleted ones. `false` when there was none.
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>>;
    /// The Projets deleted so far, most recent first.
    fn deleted(&self) -> BoxFuture<'_, StoreResult<Vec<DeletedProject>>>;
}

/// The skill context's word on a Projet's skills selection, which the
/// workspace keeps without reading it.
pub trait SkillSelections: Send + Sync {
    /// Why `next` may not replace `current` (a skill it gains the bank does
    /// not hold); `None` when it may.
    fn refusal<'a>(
        &'a self,
        current: Option<&'a StoredSetting>,
        next: &'a StoredSetting,
    ) -> BoxFuture<'a, StoreResult<Option<String>>>;
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

/// A repository a Run worked in, with the last Run that started there and
/// the Projet recorded on that Run (ADR-0087).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRepository {
    pub path: String,
    pub run_id: String,
    pub project_id: Option<String>,
}

impl RecentRepository {
    /// A repository whose last Run was launched in no Projet.
    pub fn new(path: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            run_id: run_id.into(),
            project_id: None,
        }
    }

    /// The same, its last Run launched in the Projet `project_id`.
    pub fn in_project(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
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
    /// The URL of `origin`, else of the first remote; `None` without one.
    fn remote_url(&self, repo: &Path) -> Option<String>;
    /// Create (or reuse) `<parent>/<name>` as a repository holding `files`.
    fn create_repository(
        &self,
        parent: &str,
        name: &str,
        files: &[ScaffoldFile],
    ) -> Result<ScaffoldOutcome, String>;
    /// Clone `url` in full into `dest` with the daemon's credentials, telling
    /// `on_progress` each line git prints; nothing is left of `dest` when it
    /// fails or `cancel` fires.
    fn clone_repository<'a>(
        &'a self,
        url: &'a str,
        dest: &'a Path,
        on_progress: Box<dyn Fn(&str) + Send + Sync + 'a>,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) -> BoxFuture<'a, Result<(), CloneFailure>>;
}

/// Where a repository given by its URL is cloned (an instance setting).
pub trait ClonesFolder: Send + Sync {
    /// The clones folder on disk, read fresh so a change applies at once; why
    /// not when it cannot be named (`~` with no home).
    fn folder(&self) -> BoxFuture<'_, Result<PathBuf, String>>;
    /// The home `~` stands for in a folder typed by a user.
    fn home(&self) -> Option<PathBuf>;
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
