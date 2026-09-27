//! A fake git, source tree and filesystem for the use-case tests (stage 2).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::*;
use crate::domain::branches::{BranchEntry, BranchKind, BranchList, FetchError};
use crate::domain::browse::BrowseRootError;
use crate::domain::fast_forward::{FastForwardDone, FastForwardRefusal, FastForwardRefused};
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
}

impl Git for FakeGit {
    fn validate_repository(&self, path: &str) -> Result<PathBuf, String> {
        if path.starts_with('/') {
            Ok(PathBuf::from(path))
        } else {
            Err("target_repo must be an absolute path".into())
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
    fn fetch<'a>(&'a self, repo: &'a Path) -> BoxFuture<'a, Option<FetchError>> {
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
