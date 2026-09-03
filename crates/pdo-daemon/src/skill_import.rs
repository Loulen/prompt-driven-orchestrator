//! Import skills from a **Source** into the Banque de skills (#670, spec #667,
//! CONTEXT.md §*Source*).
//!
//! A Source is a git repository URL (root, branch, or `/tree/<branch>/<path>`
//! sub-folder), an SSH URL, a `file://` URL, or a local folder. The daemon
//! clones it **shallow** with the credentials of the user running it, walks
//! every folder holding a `SKILL.md`, validates each one with the bank's single
//! gate ([`skill_bank::validate_skill_md`]) and reports the candidates with
//! their validity and their collisions against the bank. **Nothing is written**
//! before the import call, which names the candidates to take and, per name
//! collision, the explicit choice (replace / rename / skip).
//!
//! The clone lives in a temp directory the scan keeps for a while, keyed by a
//! client-chosen `scan_id`, so the import that follows copies from it without a
//! second clone; an expired or unknown id simply re-scans.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::skill_bank::{self, FolderProvenance, Provenance, Skill, SkillError, SkillFolder};

/// How long a scan's clone stays on disk for the import that follows.
const SCAN_TTL: Duration = Duration::from_secs(30 * 60);
/// A shallow clone that takes longer than this is refused, not awaited forever.
const CLONE_TIMEOUT: Duration = Duration::from_secs(180);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImportError {
    EmptySource,
    /// The input is neither a repository URL nor an absolute local folder.
    InvalidSource(String),
    LocalNotFound(String),
    /// `git clone` failed; carries git's stderr for the callout.
    CloneFailed(String),
    CloneTimeout,
    Cancelled,
    /// The `scan_id` is unknown or its clone was cleaned up; re-scan.
    ScanExpired,
    /// The folder addressed for a rescan / update has no provenance.
    NotASourceFolder,
    /// A candidate path named in an import is not in the scan.
    UnknownCandidate(String),
    /// A collision row was sent without a resolution.
    UnresolvedCollision(String),
    Skill(SkillError),
    Io(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySource => write!(f, "no source given"),
            Self::InvalidSource(input) => write!(
                f,
                "`{input}` is neither a repository URL (https://…, git@…, file://…) nor an absolute local folder"
            ),
            Self::LocalNotFound(path) => write!(f, "`{path}` is not a folder on this machine"),
            Self::CloneFailed(stderr) => write!(f, "{}", stderr.trim()),
            Self::CloneTimeout => write!(f, "the clone did not finish within {}s", CLONE_TIMEOUT.as_secs()),
            Self::Cancelled => write!(f, "scan cancelled"),
            Self::ScanExpired => write!(f, "this scan expired; scan the source again"),
            Self::NotASourceFolder => write!(f, "this folder was not imported from a source"),
            Self::UnknownCandidate(path) => write!(f, "`{path}` is not part of the scan"),
            Self::UnresolvedCollision(name) => {
                write!(f, "`{name}` is already taken: choose replace, rename or skip")
            }
            Self::Skill(error) => write!(f, "{error}"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<SkillError> for ImportError {
    fn from(error: SkillError) -> Self {
        Self::Skill(error)
    }
}

impl From<sqlx::Error> for ImportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Skill(SkillError::Storage(error.to_string()))
    }
}

impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Stable machine-readable code next to the message.
pub(crate) fn error_code(error: &ImportError) -> &'static str {
    match error {
        ImportError::EmptySource => "empty_source",
        ImportError::InvalidSource(_) => "invalid_source",
        ImportError::LocalNotFound(_) => "local_not_found",
        ImportError::CloneFailed(_) => "clone_failed",
        ImportError::CloneTimeout => "clone_timeout",
        ImportError::Cancelled => "cancelled",
        ImportError::ScanExpired => "scan_expired",
        ImportError::NotASourceFolder => "not_a_source_folder",
        ImportError::UnknownCandidate(_) => "unknown_candidate",
        ImportError::UnresolvedCollision(_) => "unresolved_collision",
        ImportError::Skill(_) => "skill",
        ImportError::Io(_) => "io",
    }
}

// ---------------------------------------------------------------------------
// Source parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceKind {
    Git,
    Local,
}

/// What the daemon understood of the typed source. Mirrored client-side for the
/// live chips; the daemon's reading is the one that counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ParsedSource {
    pub kind: SourceKind,
    /// The clone URL (git) or the absolute folder (local). This is the `url`
    /// stored in provenance.
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Sub-folder to scan, `/`-separated, `""` for the root.
    pub path: String,
    /// `owner/repo` for a forge URL, the folder name otherwise.
    pub repo: String,
    /// Default name of the Source folder: `repo`, suffixed ` · <last path segment>`.
    pub suggested_folder: String,
}

fn strip_git_suffix(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

fn last_segment(path: &str) -> Option<&str> {
    path.trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}

fn suggested_folder(repo: &str, path: &str) -> String {
    match last_segment(path) {
        Some(last) => format!("{repo} · {last}"),
        None => repo.to_string(),
    }
}

/// Parse the typed source. `home` expands a leading `~`.
pub(crate) fn parse_source(input: &str, home: Option<&Path>) -> Result<ParsedSource, ImportError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ImportError::EmptySource);
    }
    // SSH: git@host:owner/repo.git
    if input.starts_with("git@") || input.starts_with("ssh://") {
        let after_host = if let Some(rest) = input.strip_prefix("ssh://") {
            rest.split_once('/').map_or("", |x| x.1)
        } else {
            input.split_once(':').map_or("", |x| x.1)
        };
        let repo = strip_git_suffix(after_host.trim_matches('/')).to_string();
        if repo.is_empty() {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url: input.to_string(),
            git_ref: None,
            path: String::new(),
            repo: repo.clone(),
            suggested_folder: repo,
        });
    }
    if let Some(rest) = input.strip_prefix("file://") {
        let repo = last_segment(strip_git_suffix(rest))
            .unwrap_or("repo")
            .to_string();
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url: input.to_string(),
            git_ref: None,
            path: String::new(),
            repo: repo.clone(),
            suggested_folder: repo,
        });
    }
    if let Some((scheme, rest)) = input.split_once("://") {
        if scheme != "http" && scheme != "https" {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let mut parts = rest.split('/').filter(|s| !s.is_empty());
        let host = parts
            .next()
            .ok_or_else(|| ImportError::InvalidSource(input.to_string()))?;
        let segments: Vec<&str> = parts.collect();
        if segments.len() < 2 {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        let owner = segments[0];
        let repo_name = strip_git_suffix(segments[1]);
        let repo = format!("{owner}/{repo_name}");
        // GitHub: /tree/<ref>/<path>, /blob/<ref>/<file>; GitLab: /-/tree/<ref>/<path>.
        let mut tail = &segments[2..];
        if tail.first() == Some(&"-") {
            tail = &tail[1..];
        }
        let (git_ref, path) = match tail.first() {
            Some(&"tree") | Some(&"blob") if tail.len() >= 2 => {
                let is_blob = tail[0] == "blob";
                let mut path_segments: Vec<&str> = tail[2..].to_vec();
                if is_blob {
                    path_segments.pop();
                }
                (Some(tail[1].to_string()), path_segments.join("/"))
            }
            _ => (None, String::new()),
        };
        let url = format!("{scheme}://{host}/{owner}/{repo_name}");
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url,
            git_ref,
            path: path.clone(),
            repo: repo.clone(),
            suggested_folder: suggested_folder(&repo, &path),
        });
    }
    // Local folder.
    let expanded = if let Some(rest) = input.strip_prefix("~") {
        match home {
            Some(home) => home
                .join(rest.trim_start_matches('/'))
                .display()
                .to_string(),
            None => return Err(ImportError::InvalidSource(input.to_string())),
        }
    } else {
        input.to_string()
    };
    if !expanded.starts_with('/') {
        return Err(ImportError::InvalidSource(input.to_string()));
    }
    let normalised = expanded.trim_end_matches('/').to_string();
    let repo = last_segment(&normalised).unwrap_or("folder").to_string();
    Ok(ParsedSource {
        kind: SourceKind::Local,
        url: if normalised.is_empty() {
            "/".to_string()
        } else {
            normalised
        },
        git_ref: None,
        path: String::new(),
        repo: repo.clone(),
        suggested_folder: repo,
    })
}

// ---------------------------------------------------------------------------
// Clone + scan
// ---------------------------------------------------------------------------

/// One folder holding a `SKILL.md` at the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Candidate {
    /// `/`-separated path of the folder from the repository root.
    pub path: String,
    /// Frontmatter `name` when valid, the folder name otherwise.
    pub name: String,
    pub description: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Reference files besides `SKILL.md`.
    pub file_count: usize,
}

fn error_code_of(error: &SkillError) -> &'static str {
    match error {
        SkillError::NoFrontmatter => "no_frontmatter",
        SkillError::MalformedFrontmatter(_) => "malformed_frontmatter",
        SkillError::MissingName => "missing_name",
        SkillError::NameNotKebabCase(_) => "name_not_kebab_case",
        SkillError::MissingDescription => "missing_description",
        SkillError::EmptyBody => "empty_body",
        _ => "invalid",
    }
}

/// Reference files of a skill folder: everything but its top-level `SKILL.md`
/// (same rule as `skill_bank::list_files`).
fn count_files(dir: &Path) -> usize {
    fn walk(dir: &Path, top: bool, n: &mut usize) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, false, n);
                } else if !(top
                    && path.file_name().and_then(|f| f.to_str()) == Some(skill_bank::SKILL_MD))
                {
                    *n += 1;
                }
            }
        }
    }
    let mut n = 0;
    walk(dir, true, &mut n);
    n
}

fn rel_path(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root)
        .unwrap_or(dir)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Walk `root` recursively for folders holding a `SKILL.md`. A skill folder is
/// not descended into (its sub-folders are its reference files); `.git` is
/// skipped; hidden folders are **not** (skills live under `.agents/skills/`).
pub(crate) fn scan_dir(root: &Path, sub_path: &str) -> Vec<Candidate> {
    let start = if sub_path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(sub_path)
    };
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<Candidate>, depth: usize) {
        if depth > 32 {
            return;
        }
        let skill_md = dir.join(skill_bank::SKILL_MD);
        if skill_md.is_file() {
            let path = rel_path(root, dir);
            let folder_name = dir
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let candidate = match skill_bank::validate_skill_md(&content) {
                Ok(parsed) => Candidate {
                    path,
                    name: parsed.name,
                    description: parsed.description,
                    valid: true,
                    reason: None,
                    code: None,
                    file_count: count_files(dir),
                },
                Err(error) => Candidate {
                    path,
                    name: folder_name,
                    description: String::new(),
                    valid: false,
                    reason: Some(error.to_string()),
                    code: Some(error_code_of(&error).to_string()),
                    file_count: count_files(dir),
                },
            };
            out.push(candidate);
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().and_then(|f| f.to_str()) != Some(".git"))
            .collect();
        children.sort();
        for child in children {
            walk(root, &child, out, depth + 1);
        }
    }
    if start.is_dir() {
        walk(root, &start, &mut out, 0);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The clone (or the local folder) a scan produced, kept for the import.
struct ScanEntry {
    root: PathBuf,
    /// A temp clone to delete on expiry; a local folder is never deleted.
    owned: bool,
    source: ParsedSource,
    commit: Option<String>,
    created: Instant,
}

/// In-flight clones, so a cancel can kill the child.
struct InFlight {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

fn scans() -> &'static Mutex<HashMap<String, ScanEntry>> {
    static SCANS: OnceLock<Mutex<HashMap<String, ScanEntry>>> = OnceLock::new();
    SCANS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn in_flight() -> &'static Mutex<HashMap<String, InFlight>> {
    static FLIGHTS: OnceLock<Mutex<HashMap<String, InFlight>>> = OnceLock::new();
    FLIGHTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn cleanup_entry(entry: &ScanEntry) {
    if entry.owned {
        let _ = std::fs::remove_dir_all(&entry.root);
    }
}

fn expire_scans() {
    let mut guard = lock(scans());
    let expired: Vec<String> = guard
        .iter()
        .filter(|(_, e)| e.created.elapsed() > SCAN_TTL)
        .map(|(k, _)| k.clone())
        .collect();
    for key in expired {
        if let Some(entry) = guard.remove(&key) {
            cleanup_entry(&entry);
        }
    }
    // Clones left behind by a previous daemon process (the map is per-process):
    // anything named like ours, older than the TTL and not tracked, goes too.
    let tracked: std::collections::HashSet<PathBuf> =
        guard.values().map(|e| e.root.clone()).collect();
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("pdo-skill-scan-") || tracked.contains(&path) {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age > SCAN_TTL);
            if stale {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
}

/// Cancel an in-flight scan (the client also aborted its request). `true` when
/// something was running under that id.
pub(crate) fn cancel_scan(scan_id: &str) -> bool {
    let mut guard = lock(in_flight());
    match guard.get_mut(scan_id).and_then(|f| f.cancel.take()) {
        Some(tx) => {
            let _ = tx.send(());
            true
        }
        None => false,
    }
}

fn scan_temp_dir(scan_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdo-skill-scan-{scan_id}"))
}

async fn git_head(dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Shallow-clone `source` into a temp dir named after `scan_id`. Runs with
/// `GIT_TERMINAL_PROMPT=0`: a missing credential fails with git's own message
/// (the callout shows it) instead of hanging on a prompt nobody sees.
async fn clone_shallow(scan_id: &str, source: &ParsedSource) -> Result<PathBuf, ImportError> {
    let dir = scan_temp_dir(scan_id);
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    lock(in_flight()).insert(scan_id.to_string(), InFlight { cancel: Some(tx) });
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--depth", "1", "--single-branch", "--quiet"]);
    if let Some(git_ref) = &source.git_ref {
        cmd.args(["--branch", git_ref]);
    }
    cmd.arg(&source.url)
        .arg(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let result = async {
        let child = cmd
            .spawn()
            .map_err(|e| ImportError::Io(format!("cannot run git: {e}")))?;
        let waited = tokio::select! {
            out = child.wait_with_output() => out.map_err(|e| ImportError::Io(e.to_string())),
            _ = rx => Err(ImportError::Cancelled),
        };
        waited
    };
    let outcome = match tokio::time::timeout(CLONE_TIMEOUT, result).await {
        Ok(Ok(output)) => {
            if output.status.success() {
                Ok(dir.clone())
            } else {
                Err(ImportError::CloneFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        }
        Ok(Err(error)) => Err(error),
        Err(_) => Err(ImportError::CloneTimeout),
    };
    lock(in_flight()).remove(scan_id);
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    outcome
}

/// Everything a scan reports, before collisions are computed against the bank.
pub(crate) struct ScanOutcome {
    pub source: ParsedSource,
    pub commit: Option<String>,
    pub candidates: Vec<Candidate>,
    /// When the sub-path yielded nothing: the folders elsewhere in the
    /// repository that do hold skills (distinct parents), for the empty state.
    pub elsewhere: Vec<String>,
    pub elsewhere_count: usize,
}

/// Clone (or open) the source, scan it, and remember the clone under `scan_id`.
pub(crate) async fn scan(scan_id: &str, source: ParsedSource) -> Result<ScanOutcome, ImportError> {
    expire_scans();
    // Drop any previous clone under the same id.
    if let Some(previous) = lock(scans()).remove(scan_id) {
        cleanup_entry(&previous);
    }
    let (root, owned) = match source.kind {
        SourceKind::Git => (clone_shallow(scan_id, &source).await?, true),
        SourceKind::Local => {
            let path = PathBuf::from(&source.url);
            if !path.is_dir() {
                return Err(ImportError::LocalNotFound(source.url.clone()));
            }
            (path, false)
        }
    };
    let commit = git_head(&root).await;
    let candidates = scan_dir(&root, &source.path);
    let (elsewhere, elsewhere_count) = if candidates.is_empty() && !source.path.is_empty() {
        let all = scan_dir(&root, "");
        let mut parents: Vec<String> = all
            .iter()
            .map(|c| {
                c.path
                    .rsplit_once('/')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default()
            })
            .collect();
        parents.sort();
        parents.dedup();
        (parents, all.len())
    } else {
        (Vec::new(), 0)
    };
    lock(scans()).insert(
        scan_id.to_string(),
        ScanEntry {
            root,
            owned,
            source: source.clone(),
            commit: commit.clone(),
            created: Instant::now(),
        },
    );
    Ok(ScanOutcome {
        source,
        commit,
        candidates,
        elsewhere,
        elsewhere_count,
    })
}

/// Look a kept scan up. `None` when unknown or expired.
fn scan_root(scan_id: &str) -> Option<(PathBuf, ParsedSource, Option<String>)> {
    expire_scans();
    let guard = lock(scans());
    guard
        .get(scan_id)
        .filter(|e| e.root.is_dir())
        .map(|e| (e.root.clone(), e.source.clone(), e.commit.clone()))
}

// ---------------------------------------------------------------------------
// Collisions against the bank
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CandidateStatus {
    New,
    /// Another skill carries this name: replace / rename / skip.
    NameTaken,
    /// A skill imported from this very source, path and commit already exists.
    SameCommit,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExistingRef {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub folder_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScannedCandidate {
    #[serde(flatten)]
    pub candidate: Candidate,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<ExistingRef>,
}

fn folder_name_of(folders: &[SkillFolder], id: Option<&str>) -> Option<String> {
    id.and_then(|id| folders.iter().find(|f| f.id == id))
        .map(|f| f.name.clone())
}

pub(crate) async fn with_collisions(
    db: &SqlitePool,
    source: &ParsedSource,
    commit: Option<&str>,
    candidates: Vec<Candidate>,
) -> Result<Vec<ScannedCandidate>, ImportError> {
    let folders = skill_bank::list_folders(db).await?;
    let mut out = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if !candidate.valid {
            out.push(ScannedCandidate {
                candidate,
                status: CandidateStatus::Invalid,
                existing: None,
            });
            continue;
        }
        let existing = skill_bank::find_by_name_ci(db, &candidate.name).await?;
        let (status, existing) = match existing {
            None => (CandidateStatus::New, None),
            Some(skill) => {
                let same = skill.source.as_ref().is_some_and(|p| {
                    p.url == source.url
                        && p.path == candidate.path
                        && commit.is_some()
                        && p.commit.as_deref() == commit
                });
                let reference = ExistingRef {
                    id: skill.id.clone(),
                    name: skill.name.clone(),
                    folder_name: folder_name_of(&folders, skill.folder_id.as_deref()),
                    folder_id: skill.folder_id.clone(),
                };
                (
                    if same {
                        CandidateStatus::SameCommit
                    } else {
                        CandidateStatus::NameTaken
                    },
                    Some(reference),
                )
            }
        };
        out.push(ScannedCandidate {
            candidate,
            status,
            existing,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImportAction {
    /// Take it as a new skill (status `new`).
    Import,
    /// Overwrite the content of the existing skill of that name (id kept).
    Replace,
    /// Take it as a new skill under another label.
    Rename,
    Skip,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ImportItem {
    pub path: String,
    pub action: ImportAction,
    /// The new label for `rename`.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImportedRow {
    pub path: String,
    pub skill: Skill,
    pub action: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FailedRow {
    pub path: String,
    pub error: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImportReport {
    pub folder: SkillFolder,
    pub imported: Vec<ImportedRow>,
    pub failed: Vec<FailedRow>,
    pub commit: Option<String>,
}

fn provenance_for(source: &ParsedSource, commit: Option<&str>, path: &str) -> Provenance {
    Provenance {
        url: source.url.clone(),
        git_ref: source.git_ref.clone(),
        commit: commit.map(str::to_string),
        path: path.to_string(),
    }
}

fn folder_provenance_for(
    source: &ParsedSource,
    commit: Option<&str>,
    candidates: &[Candidate],
) -> FolderProvenance {
    FolderProvenance {
        url: source.url.clone(),
        git_ref: source.git_ref.clone(),
        commit: commit.map(str::to_string),
        path: source.path.clone(),
        imported_at: crate::event_log::now_iso(),
        found: candidates.len() as i64,
        invalid: candidates.iter().filter(|c| !c.valid).count() as i64,
    }
}

/// Resolve the scan a write refers to: the kept clone, or a fresh scan of the
/// same source when it expired.
async fn resolve_scan(
    scan_id: &str,
    source: &ParsedSource,
) -> Result<(PathBuf, Option<String>), ImportError> {
    if let Some((root, kept, commit)) = scan_root(scan_id) {
        if kept.url == source.url && kept.git_ref == source.git_ref {
            return Ok((root, commit));
        }
    }
    let outcome = scan(scan_id, source.clone()).await?;
    let (root, _, _) = scan_root(scan_id).ok_or(ImportError::ScanExpired)?;
    Ok((root, outcome.commit))
}

fn candidate_dir(root: &Path, path: &str) -> Result<PathBuf, ImportError> {
    if path.split('/').any(|seg| seg == "..") {
        return Err(ImportError::UnknownCandidate(path.to_string()));
    }
    let dir = if path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(path)
    };
    if !dir.join(skill_bank::SKILL_MD).is_file() {
        return Err(ImportError::UnknownCandidate(path.to_string()));
    }
    Ok(dir)
}

fn failed(path: &str, error: ImportError) -> FailedRow {
    let code = match &error {
        ImportError::Skill(inner) => match inner {
            SkillError::DuplicateName { .. } => "duplicate_name".to_string(),
            other => error_code_of(other).to_string(),
        },
        other => error_code(other).to_string(),
    };
    FailedRow {
        path: path.to_string(),
        error: error.to_string(),
        code,
    }
}

/// Where the imported skills land: an existing folder, or a new one.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ImportFolder {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Perform an import: every checked candidate with its resolution. Per-row
/// failures are reported, not fatal (the popup keeps the rows red and the
/// successes ticked). The folder receives the source's provenance at the end.
pub(crate) async fn import(
    db: &SqlitePool,
    repo_root: &Path,
    scan_id: &str,
    source: ParsedSource,
    folder: ImportFolder,
    items: Vec<ImportItem>,
) -> Result<ImportReport, ImportError> {
    let (root, commit) = resolve_scan(scan_id, &source).await?;
    let candidates = scan_dir(&root, &source.path);
    // Refuse before any write: every non-skip row must be resolvable.
    for item in &items {
        if item.action == ImportAction::Skip {
            continue;
        }
        candidate_dir(&root, &item.path)?;
        if item.action == ImportAction::Rename
            && item.name.as_deref().map(str::trim).unwrap_or("").is_empty()
        {
            return Err(ImportError::UnresolvedCollision(item.path.clone()));
        }
    }
    let (target, created) = match folder.id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => (
            skill_bank::get_folder(db, id)
                .await?
                .ok_or(SkillError::FolderNotFound)?,
            false,
        ),
        None => {
            let name = folder
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&source.suggested_folder);
            (
                skill_bank::create_folder(db, name, folder.parent_id.as_deref()).await?,
                true,
            )
        }
    };
    let mut imported = Vec::new();
    let mut failures = Vec::new();
    for item in items {
        if item.action == ImportAction::Skip {
            continue;
        }
        let dir = match candidate_dir(&root, &item.path) {
            Ok(dir) => dir,
            Err(error) => {
                failures.push(failed(&item.path, error));
                continue;
            }
        };
        let provenance = provenance_for(&source, commit.as_deref(), &item.path);
        let result: Result<(Skill, &'static str), ImportError> = match item.action {
            ImportAction::Import => skill_bank::create_from_dir(
                db,
                repo_root,
                &dir,
                None,
                Some(&target.id),
                &provenance,
            )
            .await
            .map(|s| (s, "imported"))
            .map_err(Into::into),
            ImportAction::Rename => skill_bank::create_from_dir(
                db,
                repo_root,
                &dir,
                item.name.as_deref().map(str::trim),
                Some(&target.id),
                &provenance,
            )
            .await
            .map(|s| (s, "renamed"))
            .map_err(Into::into),
            ImportAction::Replace => {
                let content = std::fs::read_to_string(dir.join(skill_bank::SKILL_MD))
                    .map_err(ImportError::from);
                match content.and_then(|c| skill_bank::validate_skill_md(&c).map_err(Into::into)) {
                    Err(error) => Err(error),
                    Ok(parsed) => match skill_bank::find_by_name_ci(db, &parsed.name).await? {
                        None => skill_bank::create_from_dir(
                            db,
                            repo_root,
                            &dir,
                            None,
                            Some(&target.id),
                            &provenance,
                        )
                        .await
                        .map(|s| (s, "imported"))
                        .map_err(Into::into),
                        Some(existing) => skill_bank::replace_content_from_dir(
                            db,
                            repo_root,
                            &existing.id,
                            &dir,
                            &provenance,
                        )
                        .await
                        .map(|s| (s, "replaced"))
                        .map_err(Into::into),
                    },
                }
            }
            ImportAction::Skip => unreachable!(),
        };
        match result {
            Ok((skill, action)) => imported.push(ImportedRow {
                path: item.path,
                skill,
                action,
            }),
            Err(error) => failures.push(failed(&item.path, error)),
        }
    }
    if imported.is_empty() && created {
        // Nothing landed: do not leave an empty Source folder behind.
        let _ = skill_bank::delete_folder(db, &target.id).await;
        return Ok(ImportReport {
            folder: target,
            imported,
            failed: failures,
            commit,
        });
    }
    let folder = skill_bank::set_folder_provenance(
        db,
        &target.id,
        &folder_provenance_for(&source, commit.as_deref(), &candidates),
    )
    .await?;
    let _ =
        skill_bank::remember_source(db, &source.url, source.git_ref.as_deref(), &source.path).await;
    Ok(ImportReport {
        folder,
        imported,
        failed: failures,
        commit,
    })
}

// ---------------------------------------------------------------------------
// Rescan + update of a Source folder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateStatus {
    /// Content differs at the source; checked by default.
    Updated,
    /// Identical content; only the provenance commit moves.
    Unchanged,
    /// At the source, not in the bank; unchecked by default.
    New,
    /// Imported from this source but moved out of this folder by the user.
    Skipped,
    /// In this folder, no longer at the source; kept and flagged.
    Gone,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpdateEntry {
    pub path: String,
    pub name: String,
    pub description: String,
    pub status: UpdateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `updated` only.
    #[serde(default)]
    pub skill_md_changed: bool,
    #[serde(default)]
    pub files_added: usize,
    #[serde(default)]
    pub files_removed: usize,
    #[serde(default)]
    pub files_changed: usize,
    /// `new` only: another skill already carries this name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_taken_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RescanReport {
    pub scan_id: String,
    pub source: ParsedSource,
    pub previous_commit: Option<String>,
    pub commit: Option<String>,
    pub entries: Vec<UpdateEntry>,
}

fn hash_tree(dir: &Path) -> HashMap<String, [u8; 32]> {
    fn walk(base: &Path, dir: &Path, out: &mut HashMap<String, [u8; 32]>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|f| f.to_str()) == Some(".git") {
                    continue;
                }
                walk(base, &path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                out.insert(rel_path(base, &path), hasher.finalize().into());
            }
        }
    }
    let mut out = HashMap::new();
    walk(dir, dir, &mut out);
    out
}

fn source_of_folder(folder: &SkillFolder) -> Result<ParsedSource, ImportError> {
    let provenance = folder
        .source
        .as_ref()
        .ok_or(ImportError::NotASourceFolder)?;
    let mut parsed = parse_source(&provenance.url, None)?;
    parsed.git_ref = provenance.git_ref.clone();
    parsed.path = provenance.path.clone();
    parsed.suggested_folder = suggested_folder(&parsed.repo, &parsed.path);
    Ok(parsed)
}

/// Re-scan a Source folder's source and diff it against the folder's skills.
pub(crate) async fn rescan(
    db: &SqlitePool,
    repo_root: &Path,
    folder_id: &str,
    scan_id: &str,
) -> Result<RescanReport, ImportError> {
    let folder = skill_bank::get_folder(db, folder_id)
        .await?
        .ok_or(SkillError::FolderNotFound)?;
    let source = source_of_folder(&folder)?;
    let previous_commit = folder.source.as_ref().and_then(|s| s.commit.clone());
    let outcome = scan(scan_id, source.clone()).await?;
    let (root, _, _) = scan_root(scan_id).ok_or(ImportError::ScanExpired)?;
    let skills = skill_bank::list(db).await?;
    let from_source: Vec<&Skill> = skills
        .iter()
        .filter(|s| s.source.as_ref().is_some_and(|p| p.url == source.url))
        .collect();
    let mut entries = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    for candidate in &outcome.candidates {
        seen_paths.insert(candidate.path.clone());
        let matched = from_source
            .iter()
            .find(|s| s.source.as_ref().is_some_and(|p| p.path == candidate.path));
        let entry = match matched {
            Some(skill) if skill.folder_id.as_deref() != Some(folder_id) => UpdateEntry {
                path: candidate.path.clone(),
                name: skill.name.clone(),
                description: candidate.description.clone(),
                status: UpdateStatus::Skipped,
                skill_id: Some(skill.id.clone()),
                reason: Some("moved out of this folder by you".to_string()),
                skill_md_changed: false,
                files_added: 0,
                files_removed: 0,
                files_changed: 0,
                name_taken_by: None,
            },
            Some(skill) => {
                if !candidate.valid {
                    UpdateEntry {
                        path: candidate.path.clone(),
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        status: UpdateStatus::Invalid,
                        skill_id: Some(skill.id.clone()),
                        reason: candidate.reason.clone(),
                        skill_md_changed: false,
                        files_added: 0,
                        files_removed: 0,
                        files_changed: 0,
                        name_taken_by: None,
                    }
                } else {
                    let here = hash_tree(&skill_bank::skill_dir(repo_root, &skill.id));
                    let there = hash_tree(&root.join(&candidate.path));
                    let mut added = 0;
                    let mut changed = 0;
                    let mut skill_md_changed = false;
                    for (path, hash) in &there {
                        match here.get(path) {
                            None => added += 1,
                            Some(h) if h != hash => {
                                if path == skill_bank::SKILL_MD {
                                    skill_md_changed = true;
                                } else {
                                    changed += 1;
                                }
                            }
                            Some(_) => {}
                        }
                    }
                    let removed = here.keys().filter(|p| !there.contains_key(*p)).count();
                    let differs = added + changed + removed > 0 || skill_md_changed;
                    UpdateEntry {
                        path: candidate.path.clone(),
                        name: skill.name.clone(),
                        description: candidate.description.clone(),
                        status: if differs {
                            UpdateStatus::Updated
                        } else {
                            UpdateStatus::Unchanged
                        },
                        skill_id: Some(skill.id.clone()),
                        reason: None,
                        skill_md_changed,
                        files_added: added,
                        files_removed: removed,
                        files_changed: changed,
                        name_taken_by: None,
                    }
                }
            }
            None => {
                if !candidate.valid {
                    UpdateEntry {
                        path: candidate.path.clone(),
                        name: candidate.name.clone(),
                        description: String::new(),
                        status: UpdateStatus::Invalid,
                        skill_id: None,
                        reason: candidate.reason.clone(),
                        skill_md_changed: false,
                        files_added: 0,
                        files_removed: 0,
                        files_changed: 0,
                        name_taken_by: None,
                    }
                } else {
                    let taken = skill_bank::find_by_name_ci(db, &candidate.name).await?;
                    UpdateEntry {
                        path: candidate.path.clone(),
                        name: candidate.name.clone(),
                        description: candidate.description.clone(),
                        status: UpdateStatus::New,
                        skill_id: None,
                        reason: None,
                        skill_md_changed: false,
                        files_added: candidate.file_count,
                        files_removed: 0,
                        files_changed: 0,
                        name_taken_by: taken.map(|s| s.name),
                    }
                }
            }
        };
        entries.push(entry);
    }
    for skill in from_source {
        if skill.folder_id.as_deref() != Some(folder_id) {
            continue;
        }
        let path = skill
            .source
            .as_ref()
            .map(|p| p.path.clone())
            .unwrap_or_default();
        if seen_paths.contains(&path) {
            continue;
        }
        entries.push(UpdateEntry {
            path,
            name: skill.name.clone(),
            description: skill.description.clone(),
            status: UpdateStatus::Gone,
            skill_id: Some(skill.id.clone()),
            reason: Some("no longer at the source · kept in the bank".to_string()),
            skill_md_changed: false,
            files_added: 0,
            files_removed: 0,
            files_changed: 0,
            name_taken_by: None,
        });
    }
    Ok(RescanReport {
        scan_id: scan_id.to_string(),
        source,
        previous_commit,
        commit: outcome.commit,
        entries,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateAction {
    /// Replace the content of the skill matched by path.
    Update,
    /// Import a skill new at the source into this folder.
    Import,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct UpdateItem {
    pub path: String,
    pub action: UpdateAction,
}

/// Apply an update after its rescan: replace the checked skills' content,
/// import the checked new ones, move every unchanged skill's commit forward,
/// and refresh the folder's provenance.
pub(crate) async fn update(
    db: &SqlitePool,
    repo_root: &Path,
    folder_id: &str,
    scan_id: &str,
    items: Vec<UpdateItem>,
) -> Result<ImportReport, ImportError> {
    let folder = skill_bank::get_folder(db, folder_id)
        .await?
        .ok_or(SkillError::FolderNotFound)?;
    let source = source_of_folder(&folder)?;
    let (root, commit) = resolve_scan(scan_id, &source).await?;
    let candidates = scan_dir(&root, &source.path);
    let skills = skill_bank::list(db).await?;
    let by_path: HashMap<String, &Skill> = skills
        .iter()
        .filter_map(|s| {
            s.source
                .as_ref()
                .filter(|p| p.url == source.url)
                .map(|p| (p.path.clone(), s))
        })
        .collect();
    let mut imported = Vec::new();
    let mut failures = Vec::new();
    let mut touched = std::collections::HashSet::new();
    for item in items {
        let dir = match candidate_dir(&root, &item.path) {
            Ok(dir) => dir,
            Err(error) => {
                failures.push(failed(&item.path, error));
                continue;
            }
        };
        let provenance = provenance_for(&source, commit.as_deref(), &item.path);
        let result: Result<(Skill, &'static str), ImportError> = match item.action {
            UpdateAction::Update => match by_path.get(&item.path) {
                Some(skill) => {
                    touched.insert(skill.id.clone());
                    skill_bank::replace_content_from_dir(
                        db,
                        repo_root,
                        &skill.id,
                        &dir,
                        &provenance,
                    )
                    .await
                    .map(|s| (s, "updated"))
                    .map_err(Into::into)
                }
                None => Err(ImportError::UnknownCandidate(item.path.clone())),
            },
            UpdateAction::Import => {
                skill_bank::create_from_dir(db, repo_root, &dir, None, Some(folder_id), &provenance)
                    .await
                    .map(|s| (s, "imported"))
                    .map_err(Into::into)
            }
        };
        match result {
            Ok((skill, action)) => imported.push(ImportedRow {
                path: item.path,
                skill,
                action,
            }),
            Err(error) => failures.push(failed(&item.path, error)),
        }
    }
    // Skills still identical at the new commit: only their provenance moves.
    for candidate in &candidates {
        if let Some(skill) = by_path.get(&candidate.path) {
            if skill.folder_id.as_deref() == Some(folder_id)
                && !touched.contains(&skill.id)
                && candidate.valid
            {
                let here = hash_tree(&skill_bank::skill_dir(repo_root, &skill.id));
                let there = hash_tree(&root.join(&candidate.path));
                if here == there {
                    let _ = skill_bank::touch_provenance(
                        db,
                        &skill.id,
                        &provenance_for(&source, commit.as_deref(), &candidate.path),
                    )
                    .await;
                }
            }
        }
    }
    let folder = skill_bank::set_folder_provenance(
        db,
        folder_id,
        &folder_provenance_for(&source, commit.as_deref(), &candidates),
    )
    .await?;
    let _ =
        skill_bank::remember_source(db, &source.url, source.git_ref.as_deref(), &source.path).await;
    Ok(ImportReport {
        folder,
        imported,
        failed: failures,
        commit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_github_root_url() {
        let parsed = parse_source("https://github.com/anthropics/skills", None).unwrap();
        assert_eq!(parsed.kind, SourceKind::Git);
        assert_eq!(parsed.url, "https://github.com/anthropics/skills");
        assert_eq!(parsed.git_ref, None);
        assert_eq!(parsed.path, "");
        assert_eq!(parsed.repo, "anthropics/skills");
        assert_eq!(parsed.suggested_folder, "anthropics/skills");
    }

    #[test]
    fn parses_a_github_tree_url_with_branch_and_sub_folder() {
        let parsed = parse_source(
            "https://github.com/anthropics/skills/tree/main/skills/engineering",
            None,
        )
        .unwrap();
        assert_eq!(parsed.url, "https://github.com/anthropics/skills");
        assert_eq!(parsed.git_ref.as_deref(), Some("main"));
        assert_eq!(parsed.path, "skills/engineering");
        assert_eq!(parsed.suggested_folder, "anthropics/skills · engineering");
    }

    #[test]
    fn parses_a_branch_only_tree_url_and_a_dot_git_suffix() {
        let parsed = parse_source("https://github.com/o/r.git/tree/dev", None).unwrap();
        assert_eq!(parsed.url, "https://github.com/o/r");
        assert_eq!(parsed.git_ref.as_deref(), Some("dev"));
        assert_eq!(parsed.path, "");
        let parsed = parse_source("https://gitlab.com/o/r/-/tree/main/skills", None).unwrap();
        assert_eq!(parsed.git_ref.as_deref(), Some("main"));
        assert_eq!(parsed.path, "skills");
    }

    #[test]
    fn parses_ssh_file_and_local_sources() {
        let ssh = parse_source("git@github.com:ippon/private-skills.git", None).unwrap();
        assert_eq!(ssh.kind, SourceKind::Git);
        assert_eq!(ssh.url, "git@github.com:ippon/private-skills.git");
        assert_eq!(ssh.repo, "ippon/private-skills");

        let file = parse_source("file:///tmp/fixture.git", None).unwrap();
        assert_eq!(file.kind, SourceKind::Git);
        assert_eq!(file.repo, "fixture");

        let local = parse_source("~/Documents/skills-repo/", Some(Path::new("/home/me"))).unwrap();
        assert_eq!(local.kind, SourceKind::Local);
        assert_eq!(local.url, "/home/me/Documents/skills-repo");
        assert_eq!(local.suggested_folder, "skills-repo");
    }

    #[test]
    fn refuses_garbage() {
        assert_eq!(
            parse_source("   ", None).unwrap_err(),
            ImportError::EmptySource
        );
        assert!(matches!(
            parse_source("not a source", None).unwrap_err(),
            ImportError::InvalidSource(_)
        ));
        assert!(matches!(
            parse_source("https://github.com/only-owner", None).unwrap_err(),
            ImportError::InvalidSource(_)
        ));
    }

    #[test]
    fn scan_finds_nested_skills_and_flags_invalid_ones() {
        let root = tempfile::tempdir().unwrap();
        let mk = |rel: &str, content: &str| {
            let dir = root.path().join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), content).unwrap();
            dir
        };
        let pdf = mk(
            "skills/engineering/pdf",
            "---\nname: pdf\ndescription: Extract text.\n---\n\nbody\n",
        );
        std::fs::write(pdf.join("ref.md"), "x").unwrap();
        std::fs::create_dir_all(pdf.join("nested")).unwrap();
        std::fs::write(
            pdf.join("nested").join("SKILL.md"),
            "not a skill: inside a skill",
        )
        .unwrap();
        mk("skills/engineering/bad", "---\nname: bad\n---\n\nbody\n");
        mk(
            ".agents/skills/hidden-ok",
            "---\nname: hidden-ok\ndescription: d\n---\n\nbody\n",
        );
        std::fs::create_dir_all(root.path().join(".git").join("x")).unwrap();
        std::fs::write(
            root.path().join(".git").join("x").join("SKILL.md"),
            "---\nname: git\ndescription: d\n---\nb",
        )
        .unwrap();

        let all = scan_dir(root.path(), "");
        let paths: Vec<&str> = all.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".agents/skills/hidden-ok",
                "skills/engineering/bad",
                "skills/engineering/pdf"
            ]
        );
        let pdf = all.iter().find(|c| c.name == "pdf").unwrap();
        assert!(pdf.valid);
        // The nested SKILL.md is a reference file of `pdf`, not a skill.
        assert_eq!(pdf.file_count, 2);
        let bad = all.iter().find(|c| c.path.ends_with("bad")).unwrap();
        assert!(!bad.valid);
        assert_eq!(bad.code.as_deref(), Some("missing_description"));

        let sub = scan_dir(root.path(), "skills/engineering");
        assert_eq!(sub.len(), 2);
        assert!(scan_dir(root.path(), "docs").is_empty());
    }
}
