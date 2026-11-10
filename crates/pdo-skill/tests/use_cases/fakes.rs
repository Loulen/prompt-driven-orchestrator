//! In-memory adapters of every port (ADR-0080, stage 2): the use cases are
//! tested against them, fast and without SQLite, a filesystem, git or a clone.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use sha2::{Digest, Sha256};

use pdo_skill::application::ports::*;
use pdo_skill::domain::delivery::{DeliveryReport, SkippedSkill};
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::import::{Candidate, ScanOutcome};
use pdo_skill::domain::import_error::ImportError;
use pdo_skill::domain::selection::ActiveSkill;
use pdo_skill::domain::skill::{FolderProvenance, Provenance, Skill, SkillFile, SkillFolder};
use pdo_skill::domain::skill_md::validate_skill_md;
use pdo_skill::domain::source::ParsedSource;
use pdo_skill::domain::update::TreeHashes;

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

/// An extension host that records every Operation and refuses them all when
/// given a reason.
#[derive(Default)]
pub struct Host {
    pub refusal: Option<String>,
    pub asked: Mutex<Vec<Operation>>,
}

impl Host {
    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }
    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        match &self.refusal {
            Some(reason) => Authorization::deny(reason.clone()),
            None => Authorization::Allow,
        }
    }
    fn publish(&self, _event: &serde_json::Value) {}
}

#[derive(Default)]
pub struct Index(pub Mutex<Vec<Skill>>);

impl Index {
    pub fn all(&self) -> Vec<Skill> {
        self.0.lock().unwrap().clone()
    }
    pub fn one(&self, id: &str) -> Option<Skill> {
        self.all().into_iter().find(|s| s.id == id)
    }
}

impl SkillIndex for Index {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Skill>>> {
        let mut all = self.all();
        all.sort_by_key(|a| a.name.to_lowercase());
        ready(Ok(all))
    }
    fn names(&self) -> BoxFuture<'_, StoreResult<BTreeMap<String, String>>> {
        ready(Ok(self.all().into_iter().map(|s| (s.id, s.name)).collect()))
    }
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>> {
        ready(Ok(self.one(id)))
    }
    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<Skill>>> {
        let wanted = name.trim().to_lowercase();
        ready(Ok(self
            .all()
            .into_iter()
            .find(|s| s.name.trim().to_lowercase() == wanted)))
    }
    fn insert<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>> {
        self.0.lock().unwrap().push(skill.clone());
        ready(Ok(()))
    }
    fn save<'a>(&'a self, skill: &'a Skill) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.id == skill.id {
                let created_at = row.created_at.clone();
                *row = skill.clone();
                row.created_at = created_at;
            }
        }
        ready(Ok(()))
    }
    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a Provenance,
    ) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.id == id {
                row.source = Some(provenance.clone());
            }
        }
        ready(Ok(()))
    }
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>> {
        let mut rows = self.0.lock().unwrap();
        let before = rows.len();
        rows.retain(|s| s.id != id);
        ready(Ok(rows.len() < before))
    }
    fn refile<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.folder_id.as_deref() == Some(from) {
                row.folder_id = to.map(str::to_string);
                row.updated_at = now.to_string();
            }
        }
        ready(Ok(()))
    }
}

#[derive(Default)]
pub struct Folders(pub Mutex<Vec<SkillFolder>>);

impl Folders {
    pub fn all(&self) -> Vec<SkillFolder> {
        self.0.lock().unwrap().clone()
    }
    pub fn one(&self, id: &str) -> Option<SkillFolder> {
        self.all().into_iter().find(|f| f.id == id)
    }
}

impl FolderIndex for Folders {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<SkillFolder>>> {
        ready(Ok(self.all()))
    }
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<SkillFolder>>> {
        ready(Ok(self.one(id)))
    }
    fn root_named<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Option<String>>> {
        let wanted = name.trim().to_lowercase();
        ready(Ok(self
            .all()
            .into_iter()
            .find(|f| f.parent_id.is_none() && f.name.trim().to_lowercase() == wanted)
            .map(|f| f.id)))
    }
    fn insert<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>> {
        self.0.lock().unwrap().push(folder.clone());
        ready(Ok(()))
    }
    fn save<'a>(&'a self, folder: &'a SkillFolder) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.id == folder.id {
                row.name = folder.name.clone();
                row.parent_id = folder.parent_id.clone();
                row.updated_at = folder.updated_at.clone();
            }
        }
        ready(Ok(()))
    }
    fn set_provenance<'a>(
        &'a self,
        id: &'a str,
        provenance: &'a FolderProvenance,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.id == id {
                row.source = Some(provenance.clone());
                row.updated_at = now.to_string();
            }
        }
        ready(Ok(()))
    }
    fn reparent<'a>(
        &'a self,
        from: &'a str,
        to: Option<&'a str>,
        now: &'a str,
    ) -> BoxFuture<'a, StoreResult<()>> {
        for row in self.0.lock().unwrap().iter_mut() {
            if row.parent_id.as_deref() == Some(from) {
                row.parent_id = to.map(str::to_string);
                row.updated_at = now.to_string();
            }
        }
        ready(Ok(()))
    }
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        self.0.lock().unwrap().retain(|f| f.id != id);
        ready(Ok(()))
    }
}

#[derive(Default)]
pub struct History(pub Mutex<Vec<ScannedSource>>);

impl SourceHistory for History {
    fn remember<'a>(&'a self, source: &'a ScannedSource) -> BoxFuture<'a, StoreResult<()>> {
        let mut rows = self.0.lock().unwrap();
        rows.retain(|s| {
            (s.url.as_str(), s.git_ref.as_str(), s.path.as_str())
                != (
                    source.url.as_str(),
                    source.git_ref.as_str(),
                    source.path.as_str(),
                )
        });
        rows.insert(0, source.clone());
        ready(Ok(()))
    }
    fn recent(&self, limit: usize) -> BoxFuture<'_, StoreResult<Vec<ScannedSource>>> {
        ready(Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .take(limit)
            .cloned()
            .collect()))
    }
}

/// A tree of files, keyed by `/`-separated path.
pub type Tree = BTreeMap<String, Vec<u8>>;

pub fn hashes_of(tree: &Tree) -> TreeHashes {
    tree.iter()
        .map(|(path, data)| (path.clone(), Sha256::digest(data).into()))
        .collect()
}

/// The bank's content, one tree per skill id. `host` holds the files of the
/// daemon's host an explorer pick may copy; `fail_writes` makes every write
/// fail like a full disk.
#[derive(Default)]
pub struct Content {
    pub trees: Mutex<BTreeMap<String, Tree>>,
    pub host: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    pub fail_writes: Mutex<bool>,
}

impl Content {
    pub fn tree(&self, id: &str) -> Option<Tree> {
        self.trees.lock().unwrap().get(id).cloned()
    }
    pub fn text(&self, id: &str, rel: &str) -> Option<String> {
        self.tree(id)
            .and_then(|t| t.get(rel).cloned())
            .map(|b| String::from_utf8(b).unwrap())
    }
    fn refuse(&self) -> StoreResult<()> {
        if *self.fail_writes.lock().unwrap() {
            return Err(SkillError::Storage("No space left on device".into()));
        }
        Ok(())
    }
}

impl SkillContent for Content {
    fn root(&self) -> PathBuf {
        PathBuf::from("/bank/.pdo/skills")
    }
    fn dir(&self, id: &str) -> PathBuf {
        self.root().join(id)
    }
    fn read_skill_md(&self, id: &str) -> StoreResult<String> {
        self.text(id, "SKILL.md")
            .ok_or_else(|| SkillError::Storage("No such file or directory".into()))
    }
    fn write_skill_md(&self, id: &str, content: &str) -> StoreResult<()> {
        self.write_file(id, "SKILL.md", content.as_bytes())
    }
    fn list_files(&self, id: &str) -> StoreResult<Vec<SkillFile>> {
        Ok(self
            .tree(id)
            .unwrap_or_default()
            .into_iter()
            .filter(|(path, _)| path != "SKILL.md")
            .map(|(path, data)| SkillFile {
                path,
                size: data.len() as u64,
            })
            .collect())
    }
    fn read_file(&self, id: &str, rel: &str) -> StoreResult<Option<Vec<u8>>> {
        Ok(self.tree(id).and_then(|t| t.get(rel).cloned()))
    }
    fn is_file(&self, id: &str, rel: &str) -> bool {
        self.tree(id).is_some_and(|t| t.contains_key(rel))
    }
    fn write_file(&self, id: &str, rel: &str, data: &[u8]) -> StoreResult<()> {
        self.refuse()?;
        self.trees
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .insert(rel.to_string(), data.to_vec());
        Ok(())
    }
    fn delete_file(&self, id: &str, rel: &str) -> StoreResult<()> {
        if let Some(tree) = self.trees.lock().unwrap().get_mut(id) {
            tree.remove(rel);
        }
        Ok(())
    }
    fn remove(&self, id: &str) -> StoreResult<()> {
        self.trees.lock().unwrap().remove(id);
        Ok(())
    }
    fn copy_from(&self, id: &str, src: &Path) -> StoreResult<()> {
        self.refuse()?;
        let tree = SOURCE_TREES
            .lock()
            .unwrap()
            .get(src)
            .cloned()
            .unwrap_or_default();
        self.trees.lock().unwrap().insert(id.to_string(), tree);
        Ok(())
    }
    fn replace_from(&self, id: &str, src: &Path) -> StoreResult<()> {
        self.copy_from(id, src)
    }
    fn hashes(&self, id: &str) -> TreeHashes {
        hashes_of(&self.tree(id).unwrap_or_default())
    }
    fn host_file_size(&self, path: &Path) -> Option<u64> {
        self.host.lock().unwrap().get(path).map(|d| d.len() as u64)
    }
    fn read_host_file(&self, path: &Path) -> StoreResult<Vec<u8>> {
        Ok(self
            .host
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default())
    }
}

/// The skill folders a fake checkout holds, by their absolute path: shared by
/// the scanner (which walks them) and the content (which copies them).
static SOURCE_TREES: Mutex<BTreeMap<PathBuf, Tree>> = Mutex::new(BTreeMap::new());

/// A Source whose checkout lives in memory: one tree per candidate path.
/// Every fake checkout is rooted at a unique path, so parallel tests never
/// share one.
pub struct Scanner {
    pub root: PathBuf,
    pub commit: Mutex<Option<String>>,
    pub candidates: Mutex<BTreeMap<String, Tree>>,
    pub kept: Mutex<HashMap<String, KeptScan>>,
    pub scans: Mutex<usize>,
    pub failure: Mutex<Option<ImportError>>,
    pub cancelled: Mutex<Vec<String>>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            root: PathBuf::from(format!("/checkout/{}", uuid::Uuid::new_v4())),
            commit: Mutex::new(Some("c1".into())),
            candidates: Mutex::default(),
            kept: Mutex::default(),
            scans: Mutex::new(0),
            failure: Mutex::new(None),
            cancelled: Mutex::default(),
        }
    }
}

impl Scanner {
    /// Put (or replace) the skill folder at `path` in the checkout.
    pub fn holds(&self, path: &str, tree: Tree) {
        SOURCE_TREES
            .lock()
            .unwrap()
            .insert(self.root.join(path), tree.clone());
        self.candidates
            .lock()
            .unwrap()
            .insert(path.to_string(), tree);
    }

    fn candidate(path: &str, tree: &Tree) -> Candidate {
        let content = String::from_utf8(tree.get("SKILL.md").cloned().unwrap_or_default()).unwrap();
        let files = tree.len().saturating_sub(1);
        match validate_skill_md(&content) {
            Ok(parsed) => Candidate {
                path: path.into(),
                name: parsed.name,
                description: parsed.description,
                valid: true,
                reason: None,
                code: None,
                file_count: files,
            },
            Err(error) => Candidate {
                path: path.into(),
                name: path.rsplit('/').next().unwrap_or(path).into(),
                description: String::new(),
                valid: false,
                reason: Some(error.to_string()),
                code: Some(error.validation_code().into()),
                file_count: files,
            },
        }
    }
}

impl SourceScanner for Scanner {
    fn scan<'a>(
        &'a self,
        scan_id: &'a str,
        source: &'a ParsedSource,
    ) -> BoxFuture<'a, Result<ScanOutcome, ImportError>> {
        *self.scans.lock().unwrap() += 1;
        if let Some(failure) = self.failure.lock().unwrap().clone() {
            return ready(Err(failure));
        }
        let commit = self.commit.lock().unwrap().clone();
        self.kept.lock().unwrap().insert(
            scan_id.to_string(),
            KeptScan {
                root: self.root.clone(),
                source: source.clone(),
                commit: commit.clone(),
            },
        );
        let candidates = self.candidates(&self.root, &source.path);
        ready(Ok(ScanOutcome {
            source: source.clone(),
            commit,
            candidates,
            elsewhere: Vec::new(),
            elsewhere_count: 0,
        }))
    }
    fn cancel(&self, scan_id: &str) -> bool {
        self.cancelled.lock().unwrap().push(scan_id.to_string());
        scan_id == "running"
    }
    fn kept(&self, scan_id: &str) -> Option<KeptScan> {
        self.kept.lock().unwrap().get(scan_id).cloned()
    }
    fn candidates(&self, _root: &Path, sub_path: &str) -> Vec<Candidate> {
        self.candidates
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with(sub_path))
            .map(|(path, tree)| Self::candidate(path, tree))
            .collect()
    }
    fn candidate_dir(&self, root: &Path, path: &str) -> Result<PathBuf, ImportError> {
        if path.split('/').any(|seg| seg == "..")
            || !self.candidates.lock().unwrap().contains_key(path)
        {
            return Err(ImportError::UnknownCandidate(path.to_string()));
        }
        Ok(root.join(path))
    }
    fn read_skill_md(&self, dir: &Path) -> std::io::Result<String> {
        SOURCE_TREES
            .lock()
            .unwrap()
            .get(dir)
            .and_then(|t| t.get("SKILL.md").cloned())
            .map(|b| String::from_utf8(b).unwrap())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }
    fn hashes(&self, dir: &Path) -> TreeHashes {
        hashes_of(
            &SOURCE_TREES
                .lock()
                .unwrap()
                .get(dir)
                .cloned()
                .unwrap_or_default(),
        )
    }
}

/// Records what it was asked to snapshot and deliver.
#[derive(Default)]
pub struct Delivery {
    pub snapshots: Mutex<Vec<(PathBuf, String, Vec<String>)>>,
    pub deliveries: Mutex<Vec<(PathBuf, PathBuf, Vec<String>)>>,
    pub withdrawn: Mutex<Vec<String>>,
    /// Ids the bank does not hold (skipped at snapshot).
    pub absent: Mutex<Vec<String>>,
    /// Names the worktree versions already (skipped at delivery).
    pub versioned: Mutex<Vec<String>>,
}

impl SkillDelivery for Delivery {
    fn snapshot(
        &self,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>> {
        self.snapshots.lock().unwrap().push((
            run_repo_root.to_path_buf(),
            run_id.to_string(),
            skills.iter().map(|s| s.id.clone()).collect(),
        ));
        let absent = self.absent.lock().unwrap().clone();
        Ok(skills
            .iter()
            .filter(|s| absent.contains(&s.id))
            .map(|s| SkippedSkill::new(s, &s.name, "skill content is absent from the bank"))
            .collect())
    }
    fn deliver(
        &self,
        worktree: &Path,
        snapshot_root: &Path,
        _run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<DeliveryReport> {
        self.deliveries.lock().unwrap().push((
            worktree.to_path_buf(),
            snapshot_root.to_path_buf(),
            skills.iter().map(|s| s.name.clone()).collect(),
        ));
        let versioned = self.versioned.lock().unwrap().clone();
        let mut report = DeliveryReport::default();
        for skill in skills {
            if versioned.contains(&skill.name) {
                report
                    .skipped
                    .push(SkippedSkill::new(skill, &skill.name, "versioned"));
            } else {
                report.delivered.push(skill.name.clone());
            }
        }
        Ok(report)
    }
    fn withdraw(&self, _repo_root: &Path, run_id: &str) -> anyhow::Result<()> {
        self.withdrawn.lock().unwrap().push(run_id.to_string());
        Ok(())
    }
}

/// The tiers downstream answer with whatever the test set.
#[derive(Default)]
pub struct Referring(pub Mutex<Referents>);

impl SkillReferents for Referring {
    fn referents<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Referents> {
        ready(self.0.lock().unwrap().clone())
    }
}

pub struct Fixture {
    pub skills: pdo_skill::Skills,
    pub index: Arc<Index>,
    pub folders: Arc<Folders>,
    pub history: Arc<History>,
    pub content: Arc<Content>,
    pub scanner: Arc<Scanner>,
    pub delivery: Arc<Delivery>,
    pub referring: Arc<Referring>,
    pub host: Arc<Host>,
}

fn build(refusal: Option<String>) -> Fixture {
    let index = Arc::new(Index::default());
    let folders = Arc::new(Folders::default());
    let history = Arc::new(History::default());
    let content = Arc::new(Content::default());
    let scanner = Arc::new(Scanner::default());
    let delivery = Arc::new(Delivery::default());
    let referring = Arc::new(Referring::default());
    let host = Arc::new(Host {
        refusal,
        asked: Mutex::default(),
    });
    let skills = pdo_skill::Skills::new(
        pdo_skill::SkillsPorts {
            skills: index.clone(),
            folders: folders.clone(),
            sources: history.clone(),
            content: content.clone(),
            scanner: scanner.clone(),
            delivery: delivery.clone(),
            referents: referring.clone(),
        },
        host.clone(),
    );
    Fixture {
        skills,
        index,
        folders,
        history,
        content,
        scanner,
        delivery,
        referring,
        host,
    }
}

/// A skill context with no extension refusing anything.
pub fn fixture() -> Fixture {
    build(None)
}

/// A skill context whose extension refuses every Operation with `reason`.
pub fn denying(reason: &str) -> Fixture {
    build(Some(reason.to_string()))
}

pub fn op() -> Actor {
    Actor::LocalOperator
}

/// A valid `SKILL.md` named `name`.
pub fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n\nBody.\n")
}

/// A skill folder holding `SKILL.md` for `name` and the given files.
pub fn folder_of(name: &str, files: &[(&str, &str)]) -> Tree {
    let mut tree = Tree::new();
    tree.insert("SKILL.md".into(), skill_md(name).into_bytes());
    for (path, body) in files {
        tree.insert(path.to_string(), body.as_bytes().to_vec());
    }
    tree
}

impl Content {
    pub fn is_file_named(&self, id: &str, rel: &str) -> bool {
        self.tree(id).is_some_and(|t| t.contains_key(rel))
    }
}
