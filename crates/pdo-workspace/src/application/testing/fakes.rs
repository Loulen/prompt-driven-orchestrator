//! A fake git, source tree and filesystem for the use-case tests (stage 2).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pdo_kernel::extension_host::GitCredentials;

use crate::application::ports::*;
use crate::domain::branches::{BranchEntry, BranchKind, BranchList, FetchError};
use crate::domain::browse::BrowseRootError;
use crate::domain::clone::CloneFailure;
use crate::domain::fast_forward::{FastForwardDone, FastForwardRefusal, FastForwardRefused};
use crate::domain::project::StoredSetting;
use crate::domain::provisioning::{ProvisioningPlan, ScopedRules};
use crate::domain::scaffold::{ScaffoldFile, ScaffoldOutcome};

use super::ready;

/// A git with one repository, `/repo`, holding a local `main` one commit
/// behind `origin/main` and an `up-to-date` branch level with its upstream.
#[derive(Default)]
pub struct FakeGit {
    fetch_error: Mutex<Option<FetchError>>,
    fetched: Mutex<Vec<String>>,
    moved: Mutex<Vec<String>>,
    created: Mutex<Vec<PathBuf>>,
    cloned: Mutex<Vec<PathBuf>>,
    /// The URLs checked by `reach`.
    reached: Mutex<Vec<String>>,
    /// The credentials of each network operation, in order.
    credentials: Mutex<Vec<GitCredentials>>,
    /// The clones wait for their cancel instead of ending.
    hold: std::sync::atomic::AtomicBool,
}

impl FakeGit {
    pub fn fail_fetch(&self, error: FetchError) {
        *self.fetch_error.lock().unwrap() = Some(error);
    }
    pub fn fetched(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }
    pub fn moved(&self) -> Vec<String> {
        self.moved.lock().unwrap().clone()
    }
    pub fn created(&self) -> Vec<PathBuf> {
        self.created.lock().unwrap().clone()
    }
    pub fn cloned(&self) -> Vec<PathBuf> {
        self.cloned.lock().unwrap().clone()
    }
    pub fn reached(&self) -> Vec<String> {
        self.reached.lock().unwrap().clone()
    }
    /// The credentials each fetch, clone and check ran with, in order.
    pub fn credentials(&self) -> Vec<GitCredentials> {
        self.credentials.lock().unwrap().clone()
    }
    pub fn hold_clones(&self) {
        self.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Git for FakeGit {
    fn validate_repository(&self, path: &str) -> Result<PathBuf, String> {
        if path.starts_with('/') {
            Ok(PathBuf::from(path))
        } else {
            Err(format!("repository must be an absolute path: {path}"))
        }
    }
    fn branches(&self, _repo: &Path) -> Result<BranchList, String> {
        Ok(BranchList {
            branches: vec![BranchEntry {
                name: "main".into(),
                kind: BranchKind::Local,
                upstream: Some("origin/main".into()),
                ahead: Some(0),
                behind: Some(1),
                last_commit_at: None,
                last_commit_subject: None,
                head: true,
            }],
            last_fetch_at: None,
            fetch_error: None,
        })
    }
    fn fetch<'a>(
        &'a self,
        repo: &'a Path,
        credentials: &'a GitCredentials,
    ) -> BoxFuture<'a, Option<FetchError>> {
        self.credentials.lock().unwrap().push(credentials.clone());
        self.fetched
            .lock()
            .unwrap()
            .push(repo.display().to_string());
        ready(self.fetch_error.lock().unwrap().clone())
    }
    fn is_local_branch(&self, _repo: &Path, branch: &str) -> bool {
        !branch.starts_with("origin/")
    }
    fn fast_forward(
        &self,
        _repo: &Path,
        branch: &str,
    ) -> Result<FastForwardDone, FastForwardRefused> {
        if branch == "up-to-date" {
            return Err(FastForwardRefused::listed(
                FastForwardRefusal::UpToDate,
                "level".into(),
            ));
        }
        self.moved.lock().unwrap().push(branch.to_string());
        Ok(FastForwardDone {
            branch: branch.to_string(),
            upstream: format!("origin/{branch}"),
            from: "a".into(),
            to: "b".into(),
            commits: 1,
            subject: None,
            moved_checkout: None,
        })
    }
    /// `/repo` is a clone of example.com/acme/repo, `/denied` of
    /// example.com/acme/denied (whose remote refuses every credential).
    fn remote_url(&self, repo: &Path) -> Option<String> {
        if repo == Path::new("/denied") {
            return Some("https://example.com/acme/denied.git".into());
        }
        (repo == Path::new("/repo")).then(|| "https://me:token@example.com/acme/repo.git".into())
    }
    fn create_repository(
        &self,
        parent: &str,
        name: &str,
        _files: &[ScaffoldFile],
    ) -> Result<ScaffoldOutcome, String> {
        if !parent.starts_with('/') {
            return Err(format!("parent must be an absolute path: {parent}"));
        }
        let path = Path::new(parent).join(name);
        self.created.lock().unwrap().push(path.clone());
        Ok(ScaffoldOutcome {
            path,
            created: true,
        })
    }
    /// Clones at once, its progress at 100 %, unless held (then it waits for
    /// its cancel); a URL naming `denied` is refused its credentials.
    fn clone_repository<'a>(
        &'a self,
        url: &'a str,
        dest: &'a Path,
        credentials: &'a GitCredentials,
        on_progress: Box<dyn Fn(&str) + Send + Sync + 'a>,
        cancel: tokio::sync::oneshot::Receiver<()>,
    ) -> BoxFuture<'a, Result<(), CloneFailure>> {
        self.credentials.lock().unwrap().push(credentials.clone());
        Box::pin(async move {
            if self.hold.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = cancel.await;
                return Err(CloneFailure::Cancelled);
            }
            if url.contains("denied") {
                return Err(CloneFailure::Failed(
                    "fatal: Authentication failed for the URL".into(),
                ));
            }
            on_progress("Receiving objects: 100% (3/3), 1.00 KiB | 1.00 KiB/s, done.");
            self.cloned.lock().unwrap().push(dest.to_path_buf());
            Ok(())
        })
    }
    /// Answers, unless the URL names `denied` (its credentials refused).
    fn reach<'a>(
        &'a self,
        url: &'a str,
        credentials: &'a GitCredentials,
    ) -> BoxFuture<'a, Result<(), CloneFailure>> {
        self.credentials.lock().unwrap().push(credentials.clone());
        self.reached.lock().unwrap().push(url.to_string());
        ready(if url.contains("denied") {
            Err(CloneFailure::Failed(
                "fatal: Authentication failed for the URL".into(),
            ))
        } else {
            Ok(())
        })
    }
}

/// The clones folder `/clones`, the home `/home/me`.
pub struct FixedClonesFolder;

impl ClonesFolder for FixedClonesFolder {
    fn folder(&self) -> BoxFuture<'_, Result<PathBuf, String>> {
        ready(Ok(PathBuf::from("/clones")))
    }
    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/home/me"))
    }
}

/// Records the last plan asked for and answers an empty one.
#[derive(Default)]
pub struct RecordingSourceTree(Mutex<Option<(PathBuf, Vec<ScopedRules>, String)>>);

impl RecordingSourceTree {
    pub fn last(&self) -> (PathBuf, Vec<ScopedRules>, String) {
        self.0
            .lock()
            .unwrap()
            .clone()
            .expect("a plan was asked for")
    }
}

impl SourceTree for RecordingSourceTree {
    fn plan(
        &self,
        repository: &Path,
        scoped: &[ScopedRules],
        git_ref: &str,
    ) -> anyhow::Result<ProvisioningPlan> {
        *self.0.lock().unwrap() = Some((
            repository.to_path_buf(),
            scoped.to_vec(),
            git_ref.to_string(),
        ));
        Ok(ProvisioningPlan::default())
    }
}

pub struct NoFiles;

impl FileBrowser for NoFiles {
    fn browse(&self, _: Option<&str>, _: bool, _: bool) -> Result<Listing, BrowseRootError> {
        Err(BrowseRootError::NoDefault)
    }
}

/// The skill context's word, scripted: refuses with `refusal` when set, and
/// records each `(current, next)` it was asked about.
#[derive(Default)]
pub struct ScriptedSkills {
    pub refusal: Mutex<Option<String>>,
    pub asked: Mutex<Vec<(Option<String>, String)>>,
}

impl SkillSelections for ScriptedSkills {
    fn refusal<'a>(
        &'a self,
        current: Option<&'a StoredSetting>,
        next: &'a StoredSetting,
    ) -> BoxFuture<'a, StoreResult<Option<String>>> {
        self.asked.lock().unwrap().push((
            current.map(|s| s.as_str().to_string()),
            next.as_str().to_string(),
        ));
        ready(Ok(self.refusal.lock().unwrap().clone()))
    }
}
