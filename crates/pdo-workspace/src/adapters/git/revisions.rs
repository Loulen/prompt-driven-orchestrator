//! A Run's revisions as git sees them: resolving a ref to its commit or tree,
//! the snapshot of a working tree, a file at a ref, and the guards a ref or a
//! path passes before it reaches `git`. Read by the run context (its refs, the
//! file at a ref, the source drift) and by the review's structured diff.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum GitError {
    /// `git` could not be spawned at all.
    Spawn(std::io::Error),
    /// `git` ran and failed; `stderr` verbatim.
    Failed { stderr: String },
}

impl GitError {
    /// The failure means one of the refs (or the repo) does not exist — the
    /// caller answers 404, not 500.
    pub fn is_unknown_revision(&self) -> bool {
        match self {
            GitError::Spawn(_) => false,
            GitError::Failed { stderr } => {
                stderr.contains("unknown revision")
                    || stderr.contains("not a git repository")
                    || stderr.contains("bad revision")
                    || stderr.contains("Not a valid object name")
                    || stderr.contains("invalid object name")
            }
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Spawn(e) => write!(f, "git diff failed: {e}"),
            GitError::Failed { stderr } => write!(f, "git diff failed: {stderr}"),
        }
    }
}

/// A ref is safe to hand to `git` as a positional argument: non-empty, no
/// leading `-` (option injection), no whitespace or control characters.
pub fn is_safe_ref(r: &str) -> bool {
    !r.is_empty() && !r.starts_with('-') && !r.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A repo-relative path is safe for `git show <ref>:<path>`: non-empty, no NUL,
/// no leading `-`, no absolute or parent-escaping components.
pub fn is_safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('-')
        && !p.starts_with('/')
        && !p.contains('\0')
        && !p.split('/').any(|seg| seg == "..")
}

pub fn run_git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    run_git_env(repo, args, &[])
}

pub fn run_git_env(repo: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Result<Vec<u8>, GitError> {
    let out = std::process::Command::new("git")
        .args(args)
        .envs(envs.iter().map(|(k, v)| (*k, *v)))
        .current_dir(repo)
        .output()
        .map_err(GitError::Spawn)?;
    if !out.status.success() {
        return Err(GitError::Failed {
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(out.stdout)
}

/// Resolve a ref to its commit SHA, `None` when it does not resolve.
pub fn rev_parse(repo: &Path, r: &str) -> Option<String> {
    let spec = format!("{r}^{{commit}}");
    run_git(repo, &["rev-parse", "--verify", "--quiet", &spec])
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve a ref to its commit SHA, else — a snapshot tree of the `worktree` ref
/// (#835) — to its tree id. `None` when it is neither.
pub fn rev_parse_object(repo: &Path, r: &str) -> Option<String> {
    rev_parse(repo, r).or_else(|| {
        let spec = format!("{r}^{{tree}}");
        run_git(repo, &["rev-parse", "--verify", "--quiet", &spec])
            .ok()
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Snapshot the working tree of `worktree` as a **tree object** (#835): the
/// tracked files as they are on disk plus the untracked, non-ignored ones,
/// `.pdo/` left out like everywhere else. Returns the tree id.
///
/// Works on a *copy* of the worktree's index (`GIT_INDEX_FILE`), so the node's
/// own staging area is never touched; `git add -A` onto that copy then
/// `write-tree` writes only the blobs that are new to the object store (a
/// dangling tree `git gc` reclaims later). Reading the Run's diff thus never
/// commits, stages or otherwise moves anything the node can observe.
pub fn snapshot_worktree(worktree: &Path) -> Result<String, GitError> {
    let index = run_git(worktree, &["rev-parse", "--git-path", "index"])?;
    let index = String::from_utf8_lossy(&index).trim().to_string();
    let index = {
        let p = PathBuf::from(&index);
        if p.is_absolute() {
            p
        } else {
            worktree.join(p)
        }
    };
    let tmp = std::env::temp_dir().join(format!(
        "pdo-snapshot-{}-{}.index",
        std::process::id(),
        SNAPSHOT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // A worktree with nothing ever staged has no index file yet: start empty.
    if index.exists() {
        std::fs::copy(&index, &tmp).map_err(GitError::Spawn)?;
    }
    let env: &[(&str, &Path)] = &[("GIT_INDEX_FILE", tmp.as_path())];
    // Stage everything, then put `.pdo/` back to its `HEAD` state. Not a
    // `:(exclude).pdo/` pathspec: naming `.pdo` in the pathspec makes `git add`
    // treat it as explicitly requested, and it aborts ("paths are ignored by one
    // of your .gitignore files") when the target repo gitignores `.pdo/` — the
    // documented setup (ADR-0060). `git reset -- .pdo` on the copied index is a
    // no-op when nothing under `.pdo` was staged or is tracked.
    let result = run_git_env(
        worktree,
        &["-c", "core.quotepath=false", "add", "-A", "--", "."],
        env,
    )
    .and_then(|_| run_git_env(worktree, &["reset", "-q", "--", ".pdo"], env))
    .and_then(|_| run_git_env(worktree, &["write-tree"], env))
    .map(|b| String::from_utf8_lossy(&b).trim().to_string());
    let _ = std::fs::remove_file(&tmp);
    result
}

static SNAPSHOT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The merge-base of `from_ref` and the worktree's `HEAD` — the left side of the
/// fork → worktree default (#835), so the source branch moving after the fork
/// never shows up as the Run's work (the three-dot semantics of [`compute`]).
pub fn merge_base_with_head(worktree: &Path, from_ref: &str) -> Result<String, GitError> {
    run_git(worktree, &["merge-base", from_ref, "HEAD"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
}

/// `git show <ref>:<path>` — the full content of one file at one ref, for the
/// context expansion of the Review page. Returns the raw bytes (a binary file
/// is a legitimate answer; the handler picks the content type).
pub fn file_at_ref(repo: &Path, r: &str, path: &str) -> Result<Vec<u8>, GitError> {
    let spec = format!("{r}:{path}");
    run_git(repo, &["-c", "core.quotepath=false", "show", &spec])
}
