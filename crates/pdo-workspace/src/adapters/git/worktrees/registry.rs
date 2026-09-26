//! What git has registered as worktrees, and the four states a node's
//! sub-worktree can be found in (#489-B).

use std::path::PathBuf;

/// What the disk says about a node's sub-worktree before a spawn touches it
/// (#489-B).
///
/// Four states, not three. The tempting three-way split — absent / reusable /
/// "blocked, so reap it" — is what makes `restart_node` destructive: a stale git
/// lock (`index.lock` from a SIGKILLed agent, a `MERGE_HEAD`) over a **dirty**
/// tree is not "already worthless", it is precisely the work #489 exists to save.
/// So *recyclable* (nothing to lose) is separated from *occupied* (someone else
/// holds it — refuse and name what holds it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubWorktreeState {
    /// Nothing on disk, no branch ref, no registration — or a directory that
    /// exists but is empty, which `git worktree add` accepts. A fresh cut.
    Absent,
    /// Registered, on the expected branch, not prunable: **reuse it in place**.
    /// This is the `restart_node` case, and the dead session's uncommitted work is
    /// still in it. Never reaped.
    Reusable {
        /// `git status --porcelain` is non-empty (untracked included) — there is
        /// something in there a reap would destroy.
        has_work: bool,
        /// The base branch is no longer an ancestor of the sub-branch: the cut is
        /// stale. Reported, never "fixed" — see [`ensure_sub_worktree`].
        base_moved: bool,
        /// Every interrupted git operation left in the worktree's private gitdir
        /// (`index.lock`, `MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`), in scan
        /// order. **All** present markers, never just the first (#516): reporting
        /// only `index.lock` once masked a coexisting `MERGE_HEAD`, and `pdo
        /// complete` then took a two-parent merge commit nobody asked for, in
        /// silence. Reported, never deleted: PDO cannot prove the writer is dead.
        interrupted_git_ops: Vec<String>,
    },
    /// Registered but prunable, or an orphaned branch ref with no worktree, or a
    /// detached checkout of our own path. Nothing here has value; reap and re-cut.
    Recyclable { detail: String },
    /// The branch is checked out in another live worktree, or the path exists as a
    /// non-empty non-worktree directory. Refuse — touching it is not ours to do.
    Occupied { detail: String },
}

impl SubWorktreeState {
    /// The interrupted git ops this state carries, in scan order. `&[]` for every
    /// state that is about to be created from scratch.
    pub fn interrupted_git_ops(&self) -> &[String] {
        match self {
            Self::Reusable {
                interrupted_git_ops,
                ..
            } => interrupted_git_ops,
            Self::Absent | Self::Recyclable { .. } | Self::Occupied { .. } => &[],
        }
    }
}

/// One `worktree list --porcelain` record.
pub struct WorktreeRecord {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub prunable: Option<String>,
}

/// Ask git — from `repo_root` — which worktrees it has registered.
///
/// **`git worktree list --porcelain` is the authoritative probe, and a
/// per-directory one is not.** Measured: `git -C <dir> rev-parse --abbrev-ref HEAD`
/// on a plain directory *inside* the repo walks up to the main worktree and
/// answers `main` — a per-directory probe lies in silence. Neither
/// `worktree list --porcelain` nor `worktree prune` existed anywhere in the daemon
/// before #489; this is its first repository-registration probe.
pub fn registered_worktrees(repo_root: &std::path::Path) -> Vec<WorktreeRecord> {
    let Ok(output) = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(rec) = current.take() {
                records.push(rec);
            }
            current = Some(WorktreeRecord {
                path: PathBuf::from(path.trim()),
                branch: None,
                prunable: None,
            });
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(rec) = current.as_mut() {
                rec.branch = Some(branch.trim().to_string());
            }
        } else if let Some(reason) = line.strip_prefix("prunable") {
            if let Some(rec) = current.as_mut() {
                rec.prunable = Some(reason.trim().to_string());
            }
        }
    }
    if let Some(rec) = current.take() {
        records.push(rec);
    }
    records
}

/// Canonicalize for comparison, falling back to the path itself when the target
/// does not exist (which is exactly the interesting case: a registered worktree
/// whose directory was deleted).
pub fn comparable(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The linked worktree's private gitdir (`<repo>/.git/worktrees/<name>`), read
/// from the `gitdir:` pointer git writes into `<dir>/.git`.
///
/// Read from the pointer file rather than derived from the basename: `.git/worktrees/`
/// is named after the **basename**, so every node collides on `iter-1` and git
/// disambiguates to `iter-11`, `iter-12`… Deriving it would probe another node's
/// gitdir.
pub fn private_gitdir(sub_worktree_dir: &std::path::Path) -> Option<PathBuf> {
    let pointer = std::fs::read_to_string(sub_worktree_dir.join(".git")).ok()?;
    let raw = pointer.lines().next()?.strip_prefix("gitdir:")?.trim();
    Some(PathBuf::from(raw))
}

/// **Every** interrupted-git-op marker present in the worktree's private gitdir,
/// in scan order — never just the first (#516).
///
/// `index.lock` stays at the head of the scan on purpose: the preamble notice
/// leans on "the first one must be removed before anything else" (the
/// `--abort`/`--continue` commands themselves need the index lock free to run).
/// Reporting only the first marker once hid a coexisting `MERGE_HEAD` behind an
/// `index.lock`, and the merge-back then took a silent two-parent commit.
pub fn interrupted_git_ops_in(sub_worktree_dir: &std::path::Path) -> Vec<String> {
    let Some(gitdir) = private_gitdir(sub_worktree_dir) else {
        return Vec::new();
    };
    ["index.lock", "MERGE_HEAD", "rebase-merge", "rebase-apply"]
        .into_iter()
        .filter(|candidate| gitdir.join(candidate).exists())
        .map(str::to_string)
        .collect()
}

pub fn dir_is_empty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}
