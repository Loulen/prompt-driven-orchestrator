//! What the skill context needs from the outside world. The composition root
//! plugs an adapter into each port; the use cases only ever see the traits.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::domain::delivery::{DeliveryReport, SkippedSkill};
use crate::domain::error::SkillError;
use crate::domain::import::{Candidate, ScanOutcome};
use crate::domain::import_error::ImportError;
use crate::domain::selection::ActiveSkill;
use crate::domain::skill::{FolderProvenance, Provenance, Skill, SkillFile, SkillFolder};
use crate::domain::source::ParsedSource;
use crate::domain::update::TreeHashes;

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A store answer; a store failure is a [`SkillError::Storage`].
pub type StoreResult<T> = Result<T, SkillError>;

/// The index of the bank's skills.
pub trait SkillIndex: Send + Sync {
    /// Every skill, by label then creation order.
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Skill>>>;
    /// The `id → name` map of the whole bank.
    fn names(&self) -> BoxFuture<'_, StoreResult<BTreeMap<String, String>>>;
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>>;
    /// Case-insensitive lookup by label, trimmed on both sides.
    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>>;
    fn insert<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>>;
    /// Rewrite the row of `skill.id`: label, description, folder, provenance,
    /// `updated_at`.
    fn save<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>>;
    /// Move the provenance forward without touching `updated_at`.
    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a Provenance,
    ) -> BoxFuture<'a, StoreResult<()>>;
    /// `false` when the id was unknown.
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>>;
    /// Every skill filed in `from` moves to `to` (a folder deletion).
    fn refile<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>>;
}

/// The index of the bank's folders.
pub trait FolderIndex: Send + Sync {
    /// Every folder, by name then creation order.
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<SkillFolder>>>;
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<SkillFolder>>>;
    /// A root folder carrying `name`, case-insensitively.
    fn root_named<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<String>>>;
    fn insert<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>>;
    /// Rewrite the name, the parent and `updated_at` of `folder.id`.
    fn save<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>>;
    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a FolderProvenance,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>>;
    /// Every folder under `from` moves under `to` (a folder deletion).
    fn reparent<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>>;
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<()>>;
}

/// A source the operator scanned, as the history keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedSource {
    pub url: String,
    /// Empty when the scan named no ref.
    pub git_ref: String,
    pub path: String,
    pub last_used_at: String,
}

/// The sources the operator has scanned, for the popup's "Recent sources".
pub trait SourceHistory: Send + Sync {
    fn remember<'a>(&'a self, source: &'a ScannedSource) -> BoxFuture<'a, StoreResult<()>>;
    /// The most recent first.
    fn recent(&self, limit: usize) -> BoxFuture<'_, StoreResult<Vec<ScannedSource>>>;
}

/// The content of the bank on disk: one folder per skill, keyed by id.
pub trait SkillContent: Send + Sync {
    /// The folder holding every skill folder.
    fn root(&self) -> PathBuf;
    /// One skill's folder.
    fn dir(&self, id: &str) -> PathBuf;
    fn read_skill_md(&self, id: &str) -> StoreResult<String>;
    /// Create the folder if needed and write its `SKILL.md`.
    fn write_skill_md(&self, id: &str, content: &str) -> StoreResult<()>;
    /// Every file under the folder except `SKILL.md`, sorted by path.
    fn list_files(&self, id: &str) -> StoreResult<Vec<SkillFile>>;
    /// The bytes of one file; `None` when it is not a file.
    fn read_file(&self, id: &str, rel: &str) -> StoreResult<Option<Vec<u8>>>;
    fn is_file(&self, id: &str, rel: &str) -> bool;
    /// Write a file, making the sub-folders it needs.
    fn write_file(&self, id: &str, rel: &str, data: &[u8]) -> StoreResult<()>;
    /// Delete a file, then prune the sub-folders it leaves empty.
    fn delete_file(&self, id: &str, rel: &str) -> StoreResult<()>;
    /// Remove the whole folder, if any.
    fn remove(&self, id: &str) -> StoreResult<()>;
    /// Copy `src` (a skill folder found at a source) as the folder of `id`.
    /// On failure nothing of it is left.
    fn copy_from(&self, id: &str, src: &Path) -> StoreResult<()>;
    /// Swap the folder of `id` for a copy of `src`; a failed copy restores it.
    fn replace_from(&self, id: &str, src: &Path) -> StoreResult<()>;
    fn hashes(&self, id: &str) -> TreeHashes;
    /// The size of a file on the daemon's host; `None` unless a regular file.
    fn host_file_size(&self, path: &Path) -> Option<u64>;
    fn read_host_file(&self, path: &Path) -> StoreResult<Vec<u8>>;
}

/// A scan kept for the import that follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptScan {
    pub root: PathBuf,
    pub source: ParsedSource,
    pub commit: Option<String>,
}

/// The Sources skills are imported from: cloned (or opened), walked, kept for
/// a while under a client-chosen scan id.
pub trait SourceScanner: Send + Sync {
    /// Clone (or open) the source, scan it, and keep it under `scan_id`.
    fn scan<'a>(
        &'a self,
        scan_id: &'a str,
        source: &'a ParsedSource,
    ) -> BoxFuture<'a, Result<ScanOutcome, ImportError>>;
    /// Kill the clone running under `scan_id`; `true` when there was one.
    fn cancel(&self, scan_id: &str) -> bool;
    /// The scan kept under `scan_id`; `None` when unknown or expired.
    fn kept(&self, scan_id: &str) -> Option<KeptScan>;
    /// The candidates under `sub_path` of a kept checkout.
    fn candidates(&self, root: &Path, sub_path: &str) -> Vec<Candidate>;
    /// The folder of the candidate at `path`, if it holds a `SKILL.md`.
    fn candidate_dir(&self, root: &Path, path: &str) -> Result<PathBuf, ImportError>;
    fn read_skill_md(&self, dir: &Path) -> std::io::Result<String>;
    fn hashes(&self, dir: &Path) -> TreeHashes;
}

/// The skills of a Run written into the worktrees its nodes work in.
pub trait SkillDelivery: Send + Sync {
    /// Freeze `skills` into the Run's snapshot, additively, from the bank.
    fn snapshot(
        &self,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>>;
    /// Write `skills` from the Run's snapshot into `worktree`, excluded from git.
    fn deliver(
        &self,
        worktree: &Path,
        snapshot_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<DeliveryReport>;
    /// Take the Run's exclusions back from the repository's `info/exclude`.
    fn withdraw(&self, repo_root: &Path, run_id: &str) -> anyhow::Result<()>;
}

/// A Projet, a Trigger or a Run that selects a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerReferent {
    pub id: String,
    pub name: String,
    pub pipeline_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineReferent {
    pub id: String,
    pub name: String,
    pub node_id: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReferent {
    pub run_id: String,
    pub pipeline_name: Option<String>,
    pub name: Option<String>,
}

/// Who selects a skill, by tier (#669).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Referents {
    pub instance: bool,
    pub projects: Vec<NamedRef>,
    pub triggers: Vec<TriggerReferent>,
    pub pipelines: Vec<PipelineReferent>,
    /// Runs not started yet: their resolution still lies ahead.
    pub runs: Vec<RunReferent>,
}

/// The tiers downstream of the bank that select skills (instance, Projets,
/// Triggers, pipelines, Runs): the contexts that own them answer.
pub trait SkillReferents: Send + Sync {
    fn referents<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Referents>;
}
