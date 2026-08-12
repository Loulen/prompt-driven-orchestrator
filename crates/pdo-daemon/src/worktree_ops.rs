//! Pure git/fs worktree lifecycle helpers.
//!
//! Carved out of the `lib.rs` god-file (issue #276, Slice-1), mirroring the
//! `run_advance` carve (#235/#275). These are the effect substrate *below*
//! layer 1 of ADR-0009: canonical path math for run/sub worktrees plus the
//! `git worktree add` / `git merge` shell-outs that create, merge, validate and
//! reap them. No `AppState`, no async, no event log, no tmux — only `&Path` /
//! `&str` / `i64` in, path math or a shell-out to `git`/`std::fs` out.
//!
//! Keep this module a pure worktree-lifecycle surface: `MergeResult` (the git
//! *effect*) belongs here; it is deliberately distinct from `MergeOutcome`
//! (`merge_action.rs` — the pure merge *decision* type). Do not conflate them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, warn};

/// Reap a sub-worktree + branch left orphaned by a spawn that aborted before
/// `NodeStarted` (#279), or a registration that has already lost all value
/// ([`SubWorktreeState::Recyclable`], #489). Best-effort throughout (mirrors
/// `cleanup_run`): a missing dir / branch is fine.
///
/// **`worktree prune` before `branch -D`, and the `remove` is unconditional**
/// (#489 / #498). The pre-#489 form skipped `worktree remove` when the directory
/// was already gone, which left git's *registration* in place — and a registered
/// worktree pins its branch:
///
/// ```text
/// $ git branch -D pdo/sub-A3
/// error: cannot delete branch 'pdo/sub-A3' used by worktree at '…/nodes/A3/iter-1'   # exit 1
/// ```
///
/// So the old reap left **both** locks standing and the next `worktree add -b`
/// still failed 255. Measured: either `worktree prune` or an unconditional
/// `worktree remove --force` clears the registration; doing the prune too is free
/// and covers the dir-already-deleted case the `remove` cannot.
pub(crate) fn reap_orphan_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
) {
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(sub_worktree_dir)
        .current_dir(repo_root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", sub_branch])
        .current_dir(repo_root)
        .output();
    // `worktree remove` unlinks the directory it knew about; a directory git never
    // registered (or one left behind by a failed remove) is ours to clear.
    if sub_worktree_dir.exists() {
        let _ = std::fs::remove_dir_all(sub_worktree_dir);
    }
    info!(
        "Reaped sub-worktree {} (branch {sub_branch}) — nothing of value was there (#279/#489)",
        sub_worktree_dir.display()
    );
}

pub(crate) fn worktree_dir_for_run(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

pub(crate) fn sub_worktree_path(
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("iter-{iter}"))
}

pub(crate) fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo/sub-{run_id}-{node_id}-iter-{iter}")
}

/// On-disk location of a secondary repo's read-only snapshot (#465, ADR-0042).
///
/// A **third sibling** of `worktree/` and `nodes/` under the Run directory:
/// `<repo_root>/.pdo/runs/<run_id>/repos/<alias>/`. Living under `repo_root`
/// (the primary) is deliberate — the sandbox already bind-mounts `repo_root` at
/// an identical path (invariant D3), so the snapshot is visible in-sandbox with
/// **no new mount**, and `remove_dir_all(<run_dir>)` at cleanup reclaims it.
///
/// `alias` MUST already be disambiguated by the caller (two secondaries with the
/// same basename would otherwise collide here) — the path never re-derives it
/// from the repo basename.
pub(crate) fn secondary_snapshot_path(repo_root: &Path, run_id: &str, alias: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("repos")
        .join(alias)
}

/// Create a node's sub-worktree and return **the commit it was cut from**.
///
/// That SHA is the whole basis of the #503 adoption rule (ADR-0036): a
/// merge-back may be resolved in the node's favour only when the pipeline
/// branch's tip is *still* this commit. Returned rather than re-derived so the
/// two spawn paths (`node_spawn`, `node_primitives`) cannot record different
/// things — or forget to record it.
pub(crate) fn create_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
) -> Result<String> {
    std::fs::create_dir_all(
        sub_worktree_dir
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", sub_branch])
        .arg(sub_worktree_dir)
        .arg(base_branch)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add for sub-worktree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add (sub) failed: {stderr}");
    }

    info!("Created sub-worktree at {}", sub_worktree_dir.display());
    rev_parse(sub_worktree_dir, "HEAD")
}

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
pub(crate) enum SubWorktreeState {
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
    pub(crate) fn interrupted_git_ops(&self) -> &[String] {
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
struct WorktreeRecord {
    path: PathBuf,
    branch: Option<String>,
    prunable: Option<String>,
}

/// Ask git — from `repo_root` — which worktrees it has registered.
///
/// **`git worktree list --porcelain` is the authoritative probe, and a
/// per-directory one is not.** Measured: `git -C <dir> rev-parse --abbrev-ref HEAD`
/// on a plain directory *inside* the repo walks up to the main worktree and
/// answers `main` — a per-directory probe lies in silence. Neither
/// `worktree list --porcelain` nor `worktree prune` existed anywhere in the daemon
/// before #489; this is its first repository-registration probe.
fn registered_worktrees(repo_root: &std::path::Path) -> Vec<WorktreeRecord> {
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
fn comparable(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The linked worktree's private gitdir (`<repo>/.git/worktrees/<name>`), read
/// from the `gitdir:` pointer git writes into `<dir>/.git`.
///
/// Read from the pointer file rather than derived from the basename: `.git/worktrees/`
/// is named after the **basename**, so every node collides on `iter-1` and git
/// disambiguates to `iter-11`, `iter-12`… Deriving it would probe another node's
/// gitdir.
fn private_gitdir(sub_worktree_dir: &std::path::Path) -> Option<PathBuf> {
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
fn interrupted_git_ops_in(sub_worktree_dir: &std::path::Path) -> Vec<String> {
    let Some(gitdir) = private_gitdir(sub_worktree_dir) else {
        return Vec::new();
    };
    ["index.lock", "MERGE_HEAD", "rebase-merge", "rebase-apply"]
        .into_iter()
        .filter(|candidate| gitdir.join(candidate).exists())
        .map(str::to_string)
        .collect()
}

fn dir_is_empty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Classify a node's sub-worktree **without mutating anything** (#489-B).
///
/// Pure read path: safe to call from the pre-kill probes of `restart_node`, which
/// is the whole point — an `Occupied` verdict must be knowable before a session is
/// destroyed.
pub(crate) fn classify_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
) -> SubWorktreeState {
    let want_ref = format!("refs/heads/{sub_branch}");
    let target = comparable(sub_worktree_dir);
    let records = registered_worktrees(repo_root);

    // The branch checked out somewhere ELSE, and that somewhere is still live.
    if let Some(other) = records
        .iter()
        .find(|r| r.branch.as_deref() == Some(&want_ref) && comparable(&r.path) != target)
    {
        if other.prunable.is_none() {
            return SubWorktreeState::Occupied {
                detail: format!(
                    "branch {sub_branch} is checked out in another worktree at {}",
                    other.path.display()
                ),
            };
        }
    }

    match records.iter().find(|r| comparable(&r.path) == target) {
        Some(own) => {
            if let Some(reason) = own.prunable.as_deref() {
                return SubWorktreeState::Recyclable {
                    detail: format!(
                        "git reports the worktree registration for {} prunable: {reason}",
                        sub_worktree_dir.display()
                    ),
                };
            }
            match own.branch.as_deref() {
                Some(branch) if branch == want_ref => SubWorktreeState::Reusable {
                    has_work: has_any_change(sub_worktree_dir),
                    base_moved: !base_is_ancestor(repo_root, base_branch, &want_ref),
                    interrupted_git_ops: interrupted_git_ops_in(sub_worktree_dir),
                },
                // Detached HEAD at our own path: only PDO ever creates a worktree
                // there, so its branch was deleted from under it. Nothing to keep.
                None => SubWorktreeState::Recyclable {
                    detail: format!(
                        "worktree at {} is detached, not on {sub_branch}",
                        sub_worktree_dir.display()
                    ),
                },
                // A live worktree on a DIFFERENT named branch. Not ours to reap.
                Some(other) => SubWorktreeState::Occupied {
                    detail: format!(
                        "worktree at {} is checked out on {other}, not {want_ref}",
                        sub_worktree_dir.display()
                    ),
                },
            }
        }
        None => {
            if sub_worktree_dir.exists() && !dir_is_empty(sub_worktree_dir) {
                return SubWorktreeState::Occupied {
                    detail: format!(
                        "{} exists and is not a registered git worktree",
                        sub_worktree_dir.display()
                    ),
                };
            }
            if branch_ref_exists(repo_root, &want_ref) {
                // The #498 shape: the branch ref outlives its worktree, and
                // `worktree add -b` refuses it (exit 255) for ever.
                return SubWorktreeState::Recyclable {
                    detail: format!(
                        "branch {sub_branch} exists with no worktree registered at {}",
                        sub_worktree_dir.display()
                    ),
                };
            }
            SubWorktreeState::Absent
        }
    }
}

/// Anything at all in the tree, **untracked included** — the opposite polarity to
/// [`worktree_has_tracked_changes`], which deliberately ignores `??` lines.
fn has_any_change(worktree_dir: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn base_is_ancestor(repo_root: &std::path::Path, base_branch: &str, sub_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", base_branch, sub_ref])
        .current_dir(repo_root)
        .output()
        .is_ok_and(|out| out.status.success())
}

fn branch_ref_exists(repo_root: &std::path::Path, full_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["show-ref", "--verify", "--quiet", full_ref])
        .current_dir(repo_root)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// What [`ensure_sub_worktree`] settled on.
#[derive(Debug)]
pub(crate) struct EnsuredSubWorktree {
    /// The state the disk was in *before* this call — what the wire reports as
    /// `reused_sub_worktree` and `interrupted_git_ops`.
    pub entry_state: SubWorktreeState,
    /// `true` iff this call ran `git worktree add`. Gates `orphan_to_reap`: a
    /// reused worktree must never be reapable by a later spawn abort.
    pub created: bool,
    /// The commit the sub-worktree is cut from (#503 / ADR-0036). Freshly read on a
    /// create, **carried over** on a reuse.
    pub base_sha: Option<String>,
}

/// Make a node's sub-worktree usable, whatever state it is in — the single
/// primitive both spawn paths call (#489-B).
///
/// Replaces the bare `create_sub_worktree` at both production sites, which failed
/// with exit 255 (`a branch named … already exists`) on **every** re-spawn of the
/// same iteration, i.e. on every `restart_node` of a `code-mutating` or `merge`
/// node.
///
/// The contract, state by state:
///
/// * [`SubWorktreeState::Absent`] → create, and the base SHA is the cut's own.
/// * [`SubWorktreeState::Reusable`] → **no mutating git call at all.** The dead
///   session's uncommitted work is the reason the restart was asked for.
/// * [`SubWorktreeState::Recyclable`] → reap, then create.
/// * [`SubWorktreeState::Occupied`] → `Err`, naming what holds it. Nothing is
///   touched.
///
/// `previous_base_sha` is the `base_sha` recorded by the **previous**
/// `NodeStarted` of this same iteration, and it is what a reuse reports. This
/// cannot be computed here, and the two obvious alternatives are worse than the
/// bug (ADR-0037 §6): re-reading `HEAD` in a reused worktree yields the *node's*
/// commit, which never equals the pipeline tip, so ADR-0036's adoption escape
/// hatch would be silently dead for every restarted node; and taking the pipeline
/// tip *at reuse time* would **arm** adoption falsely and let the node's tree
/// overwrite a sibling merged since the original cut.
///
/// Two things it deliberately does **not** do (ADR-0037, "Accepted limits"):
/// it does not refresh a stale base (measured: `git merge` into a dirty
/// sub-worktree fails, and committing first hands a fresh agent a tree full of
/// conflict markers — `node_retry` is the fresh-base tool), and it does not delete
/// a stale git lock (PDO cannot prove the writer is dead; #485 is the precedent
/// that cost).
pub(crate) fn ensure_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
    previous_base_sha: Option<&str>,
) -> Result<EnsuredSubWorktree> {
    let entry_state = classify_sub_worktree(repo_root, sub_worktree_dir, sub_branch, base_branch);
    match &entry_state {
        SubWorktreeState::Occupied { detail } => {
            anyhow::bail!(
                "sub-worktree {} is occupied: {detail}",
                sub_worktree_dir.display()
            )
        }
        SubWorktreeState::Reusable {
            has_work,
            base_moved,
            interrupted_git_ops,
        } => {
            info!(
                "Reusing sub-worktree {} in place (branch {sub_branch}): has_work={has_work}, \
                 base_moved={base_moved}, interrupted_git_ops={interrupted_git_ops:?} (#489)",
                sub_worktree_dir.display()
            );
            if *base_moved {
                warn!(
                    "Sub-worktree {} was cut from a base that has since moved; PDO does not \
                     refresh it — use node_retry for a fresh base (#489)",
                    sub_worktree_dir.display()
                );
            }
            if !interrupted_git_ops.is_empty() {
                warn!(
                    "Sub-worktree {} carries interrupted git ops ({interrupted_git_ops:?}); they \
                     are reported, not removed — the re-spawned agent resolves them before it \
                     completes, or the merge-back records a merge nobody intended (#516)",
                    sub_worktree_dir.display()
                );
            }
            Ok(EnsuredSubWorktree {
                base_sha: previous_base_sha.map(str::to_string),
                created: false,
                entry_state,
            })
        }
        SubWorktreeState::Recyclable { detail } => {
            warn!(
                "Recycling sub-worktree {} before re-cutting it: {detail} (#489/#498)",
                sub_worktree_dir.display()
            );
            reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
            let base_sha =
                create_sub_worktree(repo_root, sub_worktree_dir, sub_branch, base_branch)?;
            Ok(EnsuredSubWorktree {
                entry_state,
                created: true,
                base_sha: Some(base_sha),
            })
        }
        SubWorktreeState::Absent => {
            let base_sha =
                create_sub_worktree(repo_root, sub_worktree_dir, sub_branch, base_branch)?;
            Ok(EnsuredSubWorktree {
                entry_state,
                created: true,
                base_sha: Some(base_sha),
            })
        }
    }
}

/// Everything a conflicting merge-back knows about itself (#503, AC3/AC4).
///
/// Exists because `MergeResult::Conflict(String)` could not answer the only two
/// questions a post-mortem asks — *which files?* and *which two commits?* — and
/// the one string it did carry was empty (see [`MergeConflict::detail`]).
#[derive(Debug, Clone)]
pub(crate) struct MergeConflict {
    /// Everything `git merge` printed, **stdout first**.
    ///
    /// Pre-#503 this was `stderr` alone. `git merge` writes its whole
    /// `CONFLICT (…)` report to *stdout* and leaves stderr byte-empty on a
    /// conflict, so every `merge_conflict_detected` event PDO had ever written
    /// carried `detail: ""` — including the one for the 20-file conflict this
    /// issue came from.
    pub detail: String,
    /// The pipeline branch's tip, before the merge was attempted.
    pub pipeline_tip: String,
    /// The node's branch tip, i.e. what it asked to have merged.
    pub node_tip: String,
    /// Paths git left unmerged. Read *before* the abort restores the tree.
    pub conflicting_files: Vec<String>,
}

/// Why a conflicting merge-back may be resolved in the node's favour (#503, ADR-0036).
///
/// **Structural, not content-based**, and that is the load-bearing part: no
/// predicate over trees, blobs or paths distinguishes "the same work, rewritten"
/// from "different work". All three candidates were measured against the real
/// occurrence and all three *refuse* it (ADR-0036 §3) — including the tree-semantic
/// one, which cannot even be reached: conflicts are symmetric, so if
/// `git merge <node>` conflicted in the pipeline worktree then
/// `git merge-tree <node> <pipeline>` conflicts too.
pub(crate) const ADOPTION_RULE: &str = "the pipeline branch's tip is still the commit \
     this node's sub-worktree was cut from, so the divergence is the run's own history \
     rewritten by the node";

/// A conflict resolved in the node's favour instead of failing the run (#503).
#[derive(Debug, Clone)]
pub(crate) struct MergeAdoption {
    pub pipeline_tip: String,
    pub node_tip: String,
    /// The merge commit now at the pipeline branch's tip. Two parents — the old
    /// pipeline tip **first** — so every superseded commit stays reachable and
    /// nothing is destroyed, only outvoted on the tree.
    pub merge_commit: String,
    /// What would have conflicted, kept so the event can show the blast radius.
    pub conflicting_files: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum MergeResult {
    Success,
    /// #503: the three-way merge conflicted, but resolving it entirely in the
    /// node's favour provably drops nothing, so the pipeline branch was moved onto
    /// a merge commit carrying the node's tree. Not a failure — and never silent.
    ResolvedInNodeFavour(MergeAdoption),
    Conflict(MergeConflict),
    ConflictPendingResolution(MergeConflict),
}

/// Test shim, deliberately on the **conservative** arm: `keep_conflict` off and no
/// spawn base, so a conflict still reads `MergeResult::Conflict` here. A test that
/// wants the #503 adoption path calls `commit_and_merge_sub_worktree_inner` and
/// passes the base it cut from.
#[cfg(test)]
pub(crate) fn commit_and_merge_sub_worktree(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
) -> Result<MergeResult> {
    commit_and_merge_sub_worktree_inner(
        sub_worktree_dir,
        pipeline_worktree_dir,
        sub_branch,
        node_id,
        iter,
        false,
        None,
    )
}

/// `spawn_base`: the commit this sub-worktree was cut from, per its `NodeStarted`
/// (`merge_action::spawn_base_sha`). `None` — a pre-#503 Run, or a spawn that
/// recorded none — forbids the #503 resolution: an unknown base is not a licence to
/// rewrite a branch.
pub(crate) fn commit_and_merge_sub_worktree_inner(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
    keep_conflict: bool,
    spawn_base: Option<&str>,
) -> Result<MergeResult> {
    // #489: the exit STATUS of `git add -A`, not just the spawn. Discarding it
    // loses the whole node's work in silence, and reusing a sub-worktree (#489-B)
    // promotes that from latent to routine. The measured chain, with a leftover
    // `index.lock`:
    //
    // ```text
    // git add -A                  -> exit 128  "Unable to create '…/index.lock': File exists"
    // git diff --cached --quiet   -> exit 0    => NO COMMIT TAKEN
    // git merge pdo/sub-…         -> "Already up to date."  exit 0
    // => MergeResult::Success — the agent's file is ABSENT from the pipeline worktree
    // ```
    //
    // `pdo complete` answered `Success`, the Run went green, and 100% of the
    // uncommitted work vanished with no conflict, no event and no trace — the
    // silent loss ADR-0004 forbids. Nothing in #503 fires either: no
    // `MergeConflictDetected`, no `NodeFailed`, no `failure_reason`. `bail!` here
    // surfaces as `CompletionRefusal::MergeFailed` (a 500 the caller already
    // handles), which is loud.
    let add_output = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git add failed in sub-worktree")?;
    if !add_output.status.success() {
        anyhow::bail!(
            "git add -A in sub-worktree failed, refusing to report a merge that would drop the \
             node's work: {}",
            git_report(&add_output)
        );
    }

    let status_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git diff --cached failed")?;

    if !status_output.status.success() {
        let commit_msg = format!("{node_id} iter-{iter}: completed");
        let output = std::process::Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(sub_worktree_dir)
            .output()
            .context("git commit failed in sub-worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git commit in sub-worktree failed: {stderr}");
        }
    }

    // Both tips, read BEFORE the merge mutates either worktree (#503 AC4): a
    // post-mortem that has to rediscover these two SHAs by hand is the
    // archaeology this issue is about.
    let pipeline_tip = rev_parse(pipeline_worktree_dir, "HEAD")?;
    let node_tip = rev_parse(sub_worktree_dir, "HEAD")?;

    let output = std::process::Command::new("git")
        .args(["merge", sub_branch, "--no-edit"])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("git merge failed")?;

    if !output.status.success() {
        let conflict = MergeConflict {
            // #503 AC3: both streams, in the order git produced them.
            detail: git_report(&output),
            pipeline_tip: pipeline_tip.clone(),
            node_tip: node_tip.clone(),
            // Read while the index still holds the unmerged stages — the abort
            // below wipes them.
            conflicting_files: unmerged_paths(pipeline_worktree_dir),
        };

        if keep_conflict {
            return Ok(MergeResult::ConflictPendingResolution(conflict));
        }
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(pipeline_worktree_dir)
            .output();

        // #503 / ADR-0036: a merge-back only conflicts once the pipeline tip has
        // stopped being an ancestor of the node's branch — the invariant
        // `create_sub_worktree` establishes and a rebase inside the node breaks.
        // When resolving that divergence entirely in the node's favour provably
        // drops no work, do it: a Run that shipped must not be filed `failed`
        // over PDO's own bookkeeping.
        if adoption_allowed(pipeline_worktree_dir, &pipeline_tip, &node_tip, spawn_base)? {
            let merge_commit = resolve_in_node_favour(
                pipeline_worktree_dir,
                &pipeline_tip,
                &node_tip,
                node_id,
                iter,
            )?;
            warn!(
                "Merge-back of {sub_branch} conflicted on {} file(s) and was resolved in the \
                 node's favour: pipeline branch moved from {pipeline_tip} to {merge_commit} \
                 (tree = {node_tip}, previous tip kept as first parent). #503",
                conflict.conflicting_files.len(),
            );
            return Ok(MergeResult::ResolvedInNodeFavour(MergeAdoption {
                pipeline_tip,
                node_tip,
                merge_commit,
                conflicting_files: conflict.conflicting_files,
            }));
        }
        return Ok(MergeResult::Conflict(conflict));
    }

    // Sub-worktree and branch are intentionally kept alive (refs #32).
    // They survive until cleanup_run removes them, allowing prompt/artifact
    // inspection and tmux re-attach for completed iterations.

    info!("Merged sub-worktree {sub_branch} into pipeline branch");
    Ok(MergeResult::Success)
}

/// Resolve a revision in `dir`'s repository, or fail with git's own words.
fn rev_parse(dir: &std::path::Path, rev: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git rev-parse {rev}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse {rev} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Resolve `rev` to the SHA of a **single, unambiguous commit** in `dir`, or fail
/// (#465, ADR-0042).
///
/// Unlike bare [`rev_parse`], this passes `--verify` and peels with `^{commit}`:
/// `--verify` makes git refuse an ambiguous or partial ref instead of silently
/// picking one, and `^{commit}` rejects a ref that points at a tag/tree/blob. This
/// is what pins a secondary snapshot: the frozen SHA must be a real commit we can
/// detach onto, resolved once at Run start against the **local** ref (there is no
/// `git fetch` anywhere in the daemon — base is whatever the operator has checked
/// out). Defaults to `HEAD` at the call site.
pub(crate) fn rev_parse_verified(dir: &Path, rev: &str) -> Result<String> {
    let arg = format!("{rev}^{{commit}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &arg])
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git rev-parse --verify {arg}"))?;
    // `--verify --quiet` exits non-zero (and prints nothing) on an unresolvable or
    // ambiguous ref — the failure we want to surface with the ref's own name.
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse --verify {arg} failed: {} is not a single unambiguous commit in {}",
            rev,
            dir.display()
        );
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        anyhow::bail!(
            "git rev-parse --verify {arg} resolved to nothing in {}",
            dir.display()
        );
    }
    Ok(sha)
}

/// Everything a git invocation said, **stdout first** (#503 AC3).
///
/// `git merge` reports a conflict on stdout and says nothing on stderr, while a
/// refusal to *start* the merge ("Your local changes would be overwritten…") is
/// stderr-only. Reading one stream can only ever describe half the failures.
fn git_report(output: &std::process::Output) -> String {
    [&output.stdout, &output.stderr]
        .iter()
        .map(|raw| String::from_utf8_lossy(raw).trim_end().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The paths git left unmerged, i.e. the conflict's blast radius (#503 AC4).
///
/// Best-effort: an empty list means "git told us nothing", never "no conflict" —
/// the caller already knows the merge failed.
fn unmerged_paths(worktree_dir: &std::path::Path) -> Vec<String> {
    let Ok(output) = std::process::Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(worktree_dir)
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// May this conflict be resolved in the node's favour without dropping work?
///
/// [`ADOPTION_RULE`] is the argument: `spawn_base == pipeline_tip`. The other two
/// conditions are safety preconditions on the *mechanism*, not on the argument:
///
/// - a dirty pipeline worktree would lose uncommitted tracked work to the
///   `git reset --hard` — `git merge` fails loudly where `reset --hard` destroys
///   in silence. It can legitimately be dirty: a `doc-only`/`script` node in
///   flight, the leftovers of a `doc_violated_code_immutability` (never reverted),
///   the Run shell, the resident `__manager__` agent.
/// - unrelated histories: `git merge` also fails (`refusing to merge unrelated
///   histories`) when the two tips share no ancestor. That is not a diverged
///   pipeline branch, and adopting would replace the run's whole tree.
///
/// `false` means the conflict is genuine as far as PDO can tell, and AC2 says it
/// stays a failure.
fn adoption_allowed(
    pipeline_worktree_dir: &std::path::Path,
    pipeline_tip: &str,
    node_tip: &str,
    spawn_base: Option<&str>,
) -> Result<bool> {
    if spawn_base != Some(pipeline_tip) {
        return Ok(false);
    }
    if worktree_has_tracked_changes(pipeline_worktree_dir)? {
        return Ok(false);
    }
    Ok(share_an_ancestor(
        pipeline_worktree_dir,
        pipeline_tip,
        node_tip,
    ))
}

/// Do the two tips have a common ancestor at all?
fn share_an_ancestor(repo_dir: &std::path::Path, a: &str, b: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", a, b])
        .current_dir(repo_dir)
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

/// Move the pipeline branch onto a merge commit whose tree **is** the node's.
///
/// Not `git reset --hard <sub_branch>`: that would make the superseded commits
/// unreachable from the branch, and PDO would have quietly deleted history to fix
/// its own bookkeeping. A two-parent commit — old pipeline tip first, node tip
/// second — says exactly what happened and keeps both sides in `git log`.
fn resolve_in_node_favour(
    pipeline_worktree_dir: &std::path::Path,
    pipeline_tip: &str,
    node_tip: &str,
    node_id: &str,
    iter: i64,
) -> Result<String> {
    let node_tree = rev_parse(pipeline_worktree_dir, &format!("{node_tip}^{{tree}}"))?;
    let message = format!(
        "{node_id} iter-{iter}: merge-back resolved in the node's favour\n\n\
         The node's branch stopped being a descendant of the pipeline branch, so a\n\
         three-way merge conflicts. This commit carries the node's tree verbatim and\n\
         keeps {pipeline_tip} as its first parent, so nothing is unreachable.\n\
         \n\
         Rule: {ADOPTION_RULE}\n\
         Pipeline tip: {pipeline_tip}\n\
         Node tip:     {node_tip}\n\
         \n\
         Refs #503, ADR-0036."
    );

    let output = std::process::Command::new("git")
        .args([
            "commit-tree",
            &node_tree,
            "-p",
            pipeline_tip,
            "-p",
            node_tip,
        ])
        .args(["-m", &message])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("failed to run git commit-tree for the node-favour resolution")?;
    if !output.status.success() {
        anyhow::bail!(
            "git commit-tree failed while resolving the merge-back in the node's favour: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let merge_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let reset = std::process::Command::new("git")
        .args(["reset", "--hard", &merge_commit])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("failed to run git reset --hard onto the node-favour merge commit")?;
    if !reset.status.success() {
        anyhow::bail!(
            "git reset --hard {merge_commit} failed in the pipeline worktree: {}",
            String::from_utf8_lossy(&reset.stderr).trim()
        );
    }

    Ok(merge_commit)
}

pub(crate) fn worktree_has_tracked_changes(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .context("git status failed")?;

    let status = String::from_utf8_lossy(&output.stdout);
    Ok(status.lines().any(|line| !line.starts_with("??")))
}

/// Check that no conflict markers remain in any tracked file.
pub(crate) fn has_conflict_markers(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["grep", "-rlE", "^<{7} |^={7}$|^>{7} "])
        .current_dir(worktree_dir)
        .output()
        .context("git grep failed")?;

    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Validate merge resolution: no conflict markers, clean working tree.
pub(crate) fn validate_merge_resolution(worktree_dir: &std::path::Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();

    if has_conflict_markers(worktree_dir)? {
        problems.push("conflict markers remain in tracked files".to_string());
    }

    if worktree_has_tracked_changes(worktree_dir)? {
        problems.push("working tree is not clean (uncommitted changes)".to_string());
    }

    Ok(problems)
}

pub(crate) fn create_worktree(
    repo_root: &std::path::Path,
    worktree_dir: &std::path::Path,
    branch_name: &str,
    source_ref: &str,
) -> Result<()> {
    std::fs::create_dir_all(worktree_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name])
        .arg(worktree_dir)
        .arg(source_ref)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    info!("Created worktree at {}", worktree_dir.display());
    Ok(())
}

/// Materialise a secondary repo's **read-only snapshot** (#465, ADR-0042).
///
/// Mirror of [`create_worktree`], with one load-bearing difference: `--detach`
/// instead of `-b <branch>`. A detached worktree has **no branch**, so:
/// - there is no ref to `branch -D` at cleanup — only a registration to `prune`;
/// - it sidesteps the #498 class (a sub-worktree branch colliding with another
///   Run's), because two Runs pinning the same secondary create two detached
///   worktrees at different paths and no shared branch name.
///
/// `sha` is the frozen commit (resolved by [`rev_parse_verified`] at Run start);
/// `dest_dir` is [`secondary_snapshot_path`]. Runs with `current_dir` set to the
/// **secondary** repo (that is the repo the worktree registers into), and shares
/// its object store — the snapshot costs no full clone.
pub(crate) fn create_secondary_snapshot(
    secondary_repo: &Path,
    dest_dir: &Path,
    sha: &str,
) -> Result<()> {
    std::fs::create_dir_all(dest_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(dest_dir)
        .arg(sha)
        .current_dir(secondary_repo)
        .output()
        .context("failed to run git worktree add --detach for a secondary snapshot")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add --detach (secondary) failed: {stderr}");
    }

    info!(
        "Created secondary snapshot at {} (detached @ {sha}) from {}",
        dest_dir.display(),
        secondary_repo.display()
    );
    Ok(())
}

/// Tear down one secondary snapshot at cleanup (#465, ADR-0042).
///
/// Best-effort like [`reap_orphan_sub_worktree`] and `cleanup_run` — a missing
/// snapshot / secondary is fine — but the **`prune` is never skipped**: the
/// worktree registration lives in the *secondary* repo's `.git`, OUTSIDE
/// `repo_root`, so the Run's `remove_dir_all(<run_dir>)` cannot clear it. Without
/// the prune, every multi-repo Run leaves a dangling `--detach` registration in
/// the secondary (the #498 class), and a future `worktree add` at the same path
/// fails 255.
///
/// `current_dir` is the **secondary** repo, since that is where the registration
/// and `prune` operate.
pub(crate) fn remove_secondary_snapshot(secondary_repo: &Path, snapshot_dir: &Path) {
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(snapshot_dir)
        .current_dir(secondary_repo)
        .output();
    // The prune is the part `cleanup_run` cannot do from the primary: it clears the
    // dangling registration in the secondary's .git even if the dir is already gone.
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(secondary_repo)
        .output();
    if snapshot_dir.exists() {
        let _ = std::fs::remove_dir_all(snapshot_dir);
    }
    info!(
        "Reaped secondary snapshot {} from {} (worktree remove + prune, #465/#498)",
        snapshot_dir.display(),
        secondary_repo.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // Duplicated from lib.rs's test module (≈13 other call sites there still
    // need it) — a 14-line `git init/config/add/commit` fixture. Do not move.
    fn init_test_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "initial"]);
    }

    #[test]
    fn sub_worktree_path_follows_canonical_schema() {
        let path = sub_worktree_path(
            std::path::Path::new("/repo"),
            "20260101-120000-abc",
            "impl-1",
            1,
        );
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/runs/20260101-120000-abc/nodes/impl-1/iter-1")
        );
    }

    #[test]
    fn sub_worktree_branch_name() {
        let branch = sub_worktree_branch("20260101-120000-abc", "impl-1", 1);
        assert_eq!(branch, "pdo/sub-20260101-120000-abc-impl-1-iter-1");
    }

    #[test]
    fn cm_sub_worktree_creates_and_merges() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-run";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        assert!(sub_wt_dir.exists());

        // Make a code change in the sub-worktree
        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Verify the file is present in the pipeline worktree
        assert!(wt_dir.join("foo.rs").exists());
    }

    #[test]
    fn cm_sub_worktree_survives_after_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-survive";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Sub-worktree directory must still exist after merge (refs #32)
        assert!(
            sub_wt_dir.exists(),
            "sub-worktree directory must survive merge for inspection"
        );

        // Sub-worktree branch must still exist after merge
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branch_check.stdout);
        assert!(
            branches.contains(&sub_branch),
            "sub-branch must survive merge; got: {branches}"
        );
    }

    #[test]
    fn cm_merge_conflict_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-conflict";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Create two sub-worktrees that will conflict
        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        // Both modify the same file with different content
        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        // Merge first succeeds
        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        // Merge second → conflict
        let r2 =
            commit_and_merge_sub_worktree(&sub_wt_2, &wt_dir, &sub_branch_2, "impl-2", 1).unwrap();
        assert!(matches!(r2, MergeResult::Conflict(_)));
    }

    // ── #503 / ADR-0036 ──────────────────────────────────────────────────────
    //
    // The topology the whole issue is about, built without PDO and without an
    // agent: a run whose terminal node rebased its branch onto an integration
    // branch that moved mid-run, then re-resolved the run's own work by hand.
    // `git merge` in the pipeline worktree cannot fast-forward any more and
    // conflicts on the files both sides rewrote.

    /// Build a repo in the #503 shape and return `(repo, pipeline worktree, sub
    /// worktree, pipeline branch, sub branch)`.
    ///
    /// `node_supersedes` decides whether the node rewrites the pipeline's own
    /// change (the real occurrence: a version bump on top of it) or leaves it
    /// alone. Only the first form conflicts.
    struct RebasedNode {
        repo: PathBuf,
        pipeline_wt: PathBuf,
        sub_wt: PathBuf,
        pipeline_branch: String,
        sub_branch: String,
        /// What `create_sub_worktree` reported — the #503 guard's only input, and
        /// what a real spawn writes to `NodeStarted.base_sha`.
        spawn_base: String,
    }

    fn rebased_terminal_node_repo(tmp: &tempfile::TempDir, node_supersedes: bool) -> RebasedNode {
        let repo = tmp.path().to_path_buf();
        init_test_repo(&repo);
        let git = |dir: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed in {}: {}{}",
                dir.display(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        };
        std::fs::write(repo.join("VERSION"), "1.4.1\n").unwrap();
        git(&repo, &["add", "VERSION"]);
        git(&repo, &["commit", "-m", "base: 1.4.1"]);
        let base_branch = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // The run's pipeline worktree, and an earlier node's work merged back onto it.
        let run_id = "test-503";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(&repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
        std::fs::write(wt_dir.join("VERSION"), "1.5.0\n").unwrap();
        std::fs::write(wt_dir.join("fix.rs"), "pub fn fix() {}\n").unwrap();
        git(&wt_dir, &["add", "-A"]);
        git(&wt_dir, &["commit", "-m", "implementer: the fix — 1.5.0"]);

        // A concurrent run lands its own work on the integration branch, mid-run.
        std::fs::write(repo.join("VERSION"), "1.5.0\n").unwrap();
        std::fs::write(repo.join("other.rs"), "pub fn other() {}\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-m", "concurrent run: other — 1.5.0"]);

        // The terminal node's sub-worktree, cut from the pipeline branch tip.
        let sub_wt = sub_worktree_path(&repo, run_id, "ship", 1);
        let sub_branch = sub_worktree_branch(run_id, "ship", 1);
        let spawn_base =
            create_sub_worktree(&repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
        assert_eq!(
            spawn_base,
            rev_parse(&repo, &pipeline_branch).unwrap(),
            "the reported base must be the pipeline tip it was cut from"
        );

        // Its prompt sends it to publish against the integration branch, so it
        // rebases and resolves the incidental collision on VERSION.
        let rebase = std::process::Command::new("git")
            .args(["rebase", &base_branch])
            .current_dir(&sub_wt)
            .output()
            .unwrap();
        if !rebase.status.success() {
            std::fs::write(sub_wt.join("VERSION"), "1.5.0\n").unwrap();
            git(&sub_wt, &["add", "-A"]);
            let cont = std::process::Command::new("git")
                .args(["rebase", "--continue"])
                .env("GIT_EDITOR", "true")
                .current_dir(&sub_wt)
                .output()
                .unwrap();
            assert!(cont.status.success(), "rebase --continue failed");
        }
        if node_supersedes {
            // …and then bumps the version per its prompt, rewriting the very line
            // the pipeline branch also owns. This is what makes the merge-back
            // unrecoverable rather than a clean add/add.
            std::fs::write(sub_wt.join("VERSION"), "1.6.0\n").unwrap();
            git(&sub_wt, &["add", "-A"]);
            git(&sub_wt, &["commit", "-m", "ship it: 1.6.0"]);
        }

        RebasedNode {
            repo,
            pipeline_wt: wt_dir,
            sub_wt,
            pipeline_branch,
            sub_branch,
            spawn_base,
        }
    }

    /// AC1 — the invariant `commit_and_merge_sub_worktree_inner` relies on is
    /// broken by the node's rebase, and the merge-back must nevertheless land.
    #[test]
    fn merge_back_survives_node_rebase() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        // Precondition: the ancestry the merge-back assumed is gone.
        let ancestor = std::process::Command::new("git")
            .args([
                "merge-base",
                "--is-ancestor",
                &node.pipeline_branch,
                &node.sub_branch,
            ])
            .current_dir(&node.repo)
            .output()
            .unwrap();
        assert!(
            !ancestor.status.success(),
            "fixture must break the ancestry invariant, or it tests nothing"
        );

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            Some(&node.spawn_base),
        )
        .unwrap();

        let MergeResult::ResolvedInNodeFavour(adoption) = result else {
            panic!("expected the merge-back to be resolved in the node's favour");
        };

        // The pipeline branch now carries the node's tree, byte for byte.
        let node_tree = rev_parse(&node.repo, &format!("{}^{{tree}}", adoption.node_tip)).unwrap();
        let pipeline_tree =
            rev_parse(&node.repo, &format!("{}^{{tree}}", node.pipeline_branch)).unwrap();
        assert_eq!(
            pipeline_tree, node_tree,
            "the pipeline branch must end up on the node's tree"
        );
        assert_eq!(
            std::fs::read_to_string(node.pipeline_wt.join("VERSION")).unwrap(),
            "1.6.0\n",
            "the working tree must show the node's version, not the pipeline's"
        );
        // …including the concurrent run's file the node rebased onto.
        assert!(node.pipeline_wt.join("other.rs").exists());
        assert!(node.pipeline_wt.join("fix.rs").exists());
        assert!(!adoption.conflicting_files.is_empty());
    }

    /// Nothing is destroyed: the superseded pipeline tip is the resolution
    /// commit's FIRST parent, so `git log` on the pipeline branch still reaches it.
    #[test]
    fn resolving_in_the_node_favour_keeps_the_old_tip_reachable() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            Some(&node.spawn_base),
        )
        .unwrap();
        let MergeResult::ResolvedInNodeFavour(adoption) = result else {
            panic!("expected the merge-back to be resolved in the node's favour");
        };

        let parents = std::process::Command::new("git")
            .args(["rev-list", "--parents", "-n", "1", &adoption.merge_commit])
            .current_dir(&node.repo)
            .output()
            .unwrap();
        let parents = String::from_utf8_lossy(&parents.stdout).trim().to_string();
        let mut shas = parents.split_whitespace();
        assert_eq!(shas.next(), Some(adoption.merge_commit.as_str()));
        assert_eq!(
            shas.next(),
            Some(adoption.pipeline_tip.as_str()),
            "the superseded pipeline tip must be the FIRST parent; got: {parents}"
        );
        assert_eq!(shas.next(), Some(adoption.node_tip.as_str()));

        for tip in [&adoption.pipeline_tip, &adoption.node_tip] {
            let reachable = std::process::Command::new("git")
                .args(["merge-base", "--is-ancestor", tip, &node.pipeline_branch])
                .current_dir(&node.repo)
                .output()
                .unwrap();
            assert!(
                reachable.status.success(),
                "{tip} must stay reachable from {}",
                node.pipeline_branch
            );
        }
    }

    /// AC2 — the guard must not turn every conflict into a silent adoption. Two
    /// sibling nodes writing the same line incompatibly is a *genuine* conflict:
    /// the first one's work is on the pipeline branch and absent from the second's
    /// tree, so taking the second's tree would drop it.
    ///
    /// And this is exactly what the structural guard sees: impl-1's merge moved the
    /// pipeline tip off impl-2's spawn base.
    #[test]
    fn genuine_semantic_conflict_still_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-503-genuine";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        let base_1 = create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();
        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        let base_2 = create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();
        assert_eq!(base_1, base_2, "both siblings were cut from the same tip");

        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        let r1 = commit_and_merge_sub_worktree_inner(
            &sub_wt_1,
            &wt_dir,
            &sub_branch_1,
            "impl-1",
            1,
            false,
            Some(&base_1),
        )
        .unwrap();
        assert!(matches!(r1, MergeResult::Success));

        let r2 = commit_and_merge_sub_worktree_inner(
            &sub_wt_2,
            &wt_dir,
            &sub_branch_2,
            "impl-2",
            1,
            false,
            Some(&base_2),
        )
        .unwrap();
        assert!(
            matches!(r2, MergeResult::Conflict(_)),
            "a genuine conflict must stay a failure"
        );
        assert_eq!(
            std::fs::read_to_string(wt_dir.join("shared.txt")).unwrap(),
            "from impl-1\n",
            "impl-1's merged work must survive the refused merge-back"
        );
    }

    /// The same refusal on the #503 topology itself: a terminal node that rebased is
    /// *not* enough — if anything reached the pipeline branch after its spawn, its
    /// tree is not authoritative for the run and the conflict stands.
    #[test]
    fn a_rebased_node_whose_base_went_stale_still_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        // Something else lands on the pipeline branch after the sub-worktree was cut
        // — a doc-only node committing its docs, say.
        std::fs::write(node.pipeline_wt.join("PLAN.md"), "# plan\n").unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-m", "docs: a plan"]] {
            assert!(std::process::Command::new("git")
                .args(&args)
                .current_dir(&node.pipeline_wt)
                .output()
                .unwrap()
                .status
                .success());
        }
        assert_ne!(
            node.spawn_base,
            rev_parse(&node.repo, &node.pipeline_branch).unwrap()
        );

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            Some(&node.spawn_base),
        )
        .unwrap();
        assert!(
            matches!(result, MergeResult::Conflict(_)),
            "a stale spawn base must not be adopted — the docs commit would be dropped"
        );
        assert!(
            node.pipeline_wt.join("PLAN.md").exists(),
            "the foreign work must survive"
        );
    }

    /// A Run recorded by a pre-#503 daemon has no `base_sha`, and an unknown base is
    /// not a licence to rewrite a branch.
    #[test]
    fn an_unknown_spawn_base_is_never_adopted() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            None,
        )
        .unwrap();
        assert!(matches!(result, MergeResult::Conflict(_)));
    }

    /// AC3 — `detail` used to be `git merge`'s *stderr*, which is byte-empty on a
    /// conflict: every `merge_conflict_detected` event PDO ever wrote said nothing.
    #[test]
    fn conflict_detail_is_not_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            None,
        )
        .unwrap();
        let MergeResult::Conflict(conflict) = result else {
            panic!("expected a conflict when adoption is not allowed");
        };
        assert!(
            conflict.detail.contains("CONFLICT"),
            "detail must carry git's conflict report; got: {:?}",
            conflict.detail
        );
    }

    /// AC4 — the event must be diagnosable without archaeology: the conflicting
    /// files, and the two commits nothing else exposes.
    #[test]
    fn conflict_detail_names_the_files_and_both_tips() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        let expected_pipeline_tip = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
        let expected_node_tip = rev_parse(&node.repo, &node.sub_branch).unwrap();

        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            None,
        )
        .unwrap();
        let MergeResult::Conflict(conflict) = result else {
            panic!("expected a conflict when adoption is not allowed");
        };

        assert_eq!(conflict.pipeline_tip, expected_pipeline_tip);
        assert_eq!(conflict.node_tip, expected_node_tip);
        assert_eq!(
            conflict.conflicting_files,
            vec!["VERSION".to_string()],
            "the unmerged paths must be read before the abort wipes the index"
        );
        // And the refused merge left the pipeline worktree clean.
        assert!(!worktree_has_tracked_changes(&node.pipeline_wt).unwrap());
    }

    /// A node that rebased *cleanly* — carrying the pipeline's change forward
    /// untouched — never reaches the adoption path at all: git resolves the
    /// identical add/add itself. Pinned so the fix is not credited with a case it
    /// does not handle, and so a future change that starts rewriting branches here
    /// shows up as a failure.
    #[test]
    fn a_clean_node_rebase_needs_no_resolution_at_all() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, false);

        let before = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
        let result = commit_and_merge_sub_worktree_inner(
            &node.sub_wt,
            &node.pipeline_wt,
            &node.sub_branch,
            "ship",
            1,
            false,
            Some(&node.spawn_base),
        )
        .unwrap();
        assert!(
            matches!(result, MergeResult::Success),
            "a clean rebase merges by itself"
        );
        assert_ne!(
            rev_parse(&node.repo, &node.pipeline_branch).unwrap(),
            before
        );
        assert!(node.pipeline_wt.join("other.rs").exists());
    }

    /// The mechanism's own precondition: adoption must not fire on a pipeline
    /// worktree with uncommitted tracked work, because it ends in `reset --hard` —
    /// which destroys in silence where `git merge` fails loudly.
    #[test]
    fn adoption_refuses_a_dirty_pipeline_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let node = rebased_terminal_node_repo(&tmp, true);

        let pipeline_tip = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
        let node_tip = rev_parse(&node.repo, &node.sub_branch).unwrap();
        std::fs::write(
            node.pipeline_wt.join("fix.rs"),
            "hand-edited, uncommitted\n",
        )
        .unwrap();

        assert!(!adoption_allowed(
            &node.pipeline_wt,
            &pipeline_tip,
            &node_tip,
            Some(&node.spawn_base),
        )
        .unwrap());
    }

    /// Unrelated histories are not a diverged pipeline branch — `git merge` fails
    /// there too, and adopting would replace the run's whole tree.
    #[test]
    fn adoption_refuses_unrelated_histories() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);
        let ours = rev_parse(repo, "HEAD").unwrap();

        // A parentless commit: a second root, i.e. a history that shares nothing
        // with ours. Built with `commit-tree` so the worktree stays untouched — the
        // dirty-tree precondition must not be what this test happens to trip on.
        let tree = rev_parse(repo, "HEAD^{tree}").unwrap();
        let alien = std::process::Command::new("git")
            .args(["commit-tree", &tree, "-m", "unrelated root"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(alien.status.success());
        let alien = String::from_utf8_lossy(&alien.stdout).trim().to_string();
        assert!(!share_an_ancestor(repo, &ours, &alien));

        // The structural rule holds — the base IS the tip — and it is still refused.
        assert!(!adoption_allowed(repo, &ours, &alien, Some(&ours)).unwrap());
    }

    #[test]
    fn doc_only_clean_worktree_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-clean";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_dirty_worktree_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-dirty";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Modify a tracked file
        std::fs::write(wt_dir.join("README.md"), "# modified\n").unwrap();

        assert!(worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_untracked_files_not_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-untracked";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Add an untracked file (like artifacts)
        let port_dir = wt_dir.join(".pdo/artifacts/planner/iter-1/plan");
        std::fs::create_dir_all(&port_dir).unwrap();
        std::fs::write(port_dir.join("output.md"), "# plan\n").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn validate_merge_resolution_clean_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.is_empty(),
            "clean repo should pass validation, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_conflict_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(
            repo.join("conflict.txt"),
            "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nafter\n",
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "conflict.txt"])
            .current_dir(repo)
            .output();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("conflict markers")),
            "should detect conflict markers, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(repo.join("README.md"), "# modified\n").unwrap();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("not clean")),
            "should detect dirty worktree, got: {problems:?}"
        );
    }

    #[test]
    fn conflict_pending_resolution_keeps_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-pending";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        let r2 = commit_and_merge_sub_worktree_inner(
            &sub_wt_2,
            &wt_dir,
            &sub_branch_2,
            "impl-2",
            1,
            true,
            None,
        )
        .unwrap();
        assert!(
            matches!(r2, MergeResult::ConflictPendingResolution(_)),
            "expected ConflictPendingResolution"
        );

        // Conflict markers should remain in worktree (merge NOT aborted)
        let content = std::fs::read_to_string(wt_dir.join("shared.txt")).unwrap();
        assert!(
            content.contains("<<<<<<<"),
            "conflict markers should remain in the file"
        );
    }

    #[test]
    fn create_worktree_with_source_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        // Create a feature branch with a file
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap()
        };
        run(&["checkout", "-b", "feature-branch"]);
        std::fs::write(repo.join("feature.txt"), "feature content\n").unwrap();
        run(&["add", "feature.txt"]);
        run(&["commit", "-m", "add feature"]);
        // Go back to default branch
        let default_out = std::process::Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(repo)
            .output()
            .unwrap();
        let branch_list = String::from_utf8_lossy(&default_out.stdout).to_string();
        let default_branch = branch_list
            .trim()
            .lines()
            .find(|b| *b != "feature-branch")
            .unwrap_or("master");
        run(&["checkout", default_branch]);

        // Create worktree from feature-branch
        let wt_dir = repo
            .join(".pdo")
            .join("runs")
            .join("test-run")
            .join("worktree");
        create_worktree(repo, &wt_dir, "pdo/run-test-run", "feature-branch").unwrap();

        // The worktree should contain feature.txt from the feature branch
        assert!(wt_dir.join("feature.txt").exists());
        assert_eq!(
            std::fs::read_to_string(wt_dir.join("feature.txt")).unwrap(),
            "feature content\n"
        );
    }

    #[test]
    fn worktree_dir_for_run_follows_canonical_schema() {
        let path =
            worktree_dir_for_run(std::path::Path::new("/target-repo"), "20260101-120000-abc");
        assert_eq!(
            path,
            PathBuf::from("/target-repo/.pdo/runs/20260101-120000-abc/worktree")
        );
    }

    // New per-module unit test (#276 AC "new per-module unit tests"):
    // reap_orphan_sub_worktree was previously covered only end-to-end by
    // crates/pdo-daemon/tests/spawn_abort_recovery.rs.
    #[test]
    fn reap_orphan_sub_worktree_removes_dir_and_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-reap-orphan";
        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, "HEAD").unwrap();

        // Precondition: worktree dir + branch exist.
        assert!(sub_wt_dir.exists());
        let before = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&before.stdout).contains(&sub_branch),
            "sub-branch should exist before reap"
        );

        reap_orphan_sub_worktree(repo, &sub_wt_dir, &sub_branch);

        // Postcondition: dir gone, branch deleted.
        assert!(!sub_wt_dir.exists(), "sub-worktree dir must be removed");
        let after = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&after.stdout).trim().is_empty(),
            "sub-branch must be deleted after reap"
        );
    }

    // ── #489-B / ADR-0037 : classify + ensure ────────────────────────────────
    //
    // The six locks a re-spawn can hit were MEASURED, and the #497 corollary ("it
    // is the branch ref that blocks, not the directory") is true for its own
    // incident and false in general:
    //
    // | starting state                                   | `add -b B dir base` |
    // |--------------------------------------------------|---------------------|
    // | nothing                                          | ✅ 0                |
    // | branch + dir + registered ← the `restart_node` case | ❌ 255 branch exists |
    // | branch, dir deleted, still registered            | ❌ 255              |
    // | branch, dir absent and unregistered ← the #498 case | ❌ 255           |
    // | branch deleted, dir non-empty                    | ❌ 128 dir exists   |
    // | branch checked out in another live worktree      | ❌ 255              |

    /// A repo with a Run's pipeline worktree, ready to cut sub-worktrees from.
    fn repo_with_pipeline_branch(tmp: &tempfile::TempDir, run_id: &str) -> (PathBuf, String) {
        let repo = tmp.path().to_path_buf();
        init_test_repo(&repo);
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(&repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
        (repo, pipeline_branch)
    }

    #[test]
    fn classify_reports_absent_when_nothing_is_there() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-absent");
        let dir = sub_worktree_path(&repo, "cls-absent", "impl-1", 1);
        let branch = sub_worktree_branch("cls-absent", "impl-1", 1);
        assert_eq!(
            classify_sub_worktree(&repo, &dir, &branch, &base),
            SubWorktreeState::Absent
        );
    }

    /// **THE #489 case.** A sub-worktree registered on its own branch is reusable,
    /// and its uncommitted work is exactly why.
    #[test]
    fn classify_reports_reusable_and_sees_the_work_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-reuse");
        let dir = sub_worktree_path(&repo, "cls-reuse", "impl-1", 1);
        let branch = sub_worktree_branch("cls-reuse", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();

        let clean = classify_sub_worktree(&repo, &dir, &branch, &base);
        assert_eq!(
            clean,
            SubWorktreeState::Reusable {
                has_work: false,
                base_moved: false,
                interrupted_git_ops: vec![],
            }
        );

        // Untracked counts (the opposite polarity to `worktree_has_tracked_changes`):
        // an agent's scratch files are work a reap would destroy.
        std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();
        let dirty = classify_sub_worktree(&repo, &dir, &branch, &base);
        assert_eq!(
            dirty,
            SubWorktreeState::Reusable {
                has_work: true,
                base_moved: false,
                interrupted_git_ops: vec![],
            }
        );
    }

    /// Interrupted git ops are REPORTED, and the worktree stays reusable. Refusing
    /// here would remove the last recovery lever on a state the restart can improve;
    /// and deleting the markers ourselves is what git warns against — PDO cannot
    /// prove the writer is dead (#485 is the precedent that cost).
    ///
    /// **THE #516 case.** Both an `index.lock` AND a `MERGE_HEAD` are planted, and
    /// **both** must surface, in scan order. The pre-#516 scanner returned at the
    /// first marker, hiding the `MERGE_HEAD` behind the `index.lock` — the agent
    /// cleared the lock it was told about, ran `pdo complete`, and the merge-back
    /// took a silent two-parent commit.
    #[test]
    fn classify_reports_every_interrupted_git_op_without_refusing_the_reuse() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-lock");
        let dir = sub_worktree_path(&repo, "cls-lock", "impl-1", 1);
        let branch = sub_worktree_branch("cls-lock", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
        std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();

        // The gitdir is read from the `gitdir:` pointer, never derived from the
        // basename: `.git/worktrees/` is named by basename, so every node collides
        // on `iter-1` and git disambiguates to `iter-11`, `iter-12`…
        let gitdir = private_gitdir(&dir).expect("a linked worktree has a gitdir pointer");

        // A single marker still surfaces as a one-element Vec.
        std::fs::write(gitdir.join("index.lock"), "").unwrap();
        let one = classify_sub_worktree(&repo, &dir, &branch, &base);
        assert_eq!(one.interrupted_git_ops(), ["index.lock"]);

        // #516: a coexisting `MERGE_HEAD` must NOT be masked by the `index.lock`.
        std::fs::write(gitdir.join("MERGE_HEAD"), "").unwrap();
        let both = classify_sub_worktree(&repo, &dir, &branch, &base);
        assert_eq!(
            both,
            SubWorktreeState::Reusable {
                has_work: true,
                base_moved: false,
                interrupted_git_ops: vec!["index.lock".into(), "MERGE_HEAD".into()],
            }
        );
        // Scan order is load-bearing: `index.lock` first (the notice says "remove it
        // before anything else").
        assert_eq!(both.interrupted_git_ops(), ["index.lock", "MERGE_HEAD"]);
    }

    /// The #498 shape: the branch ref outlives its worktree, so `worktree add -b`
    /// refuses it (exit 255) for ever. Nothing of value is on disk → recycle.
    #[test]
    fn classify_recycles_an_orphaned_branch_with_no_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-orphan");
        let dir = sub_worktree_path(&repo, "cls-orphan", "impl-1", 1);
        let branch = sub_worktree_branch("cls-orphan", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
        // A worktree removed cleanly, leaving only the branch behind.
        std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&dir)
            .current_dir(&repo)
            .output()
            .unwrap();

        assert!(matches!(
            classify_sub_worktree(&repo, &dir, &branch, &base),
            SubWorktreeState::Recyclable { .. }
        ));
    }

    /// A registration whose directory was `rm -rf`ed is prunable → recycle.
    #[test]
    fn classify_recycles_a_prunable_registration() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-prunable");
        let dir = sub_worktree_path(&repo, "cls-prunable", "impl-1", 1);
        let branch = sub_worktree_branch("cls-prunable", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let state = classify_sub_worktree(&repo, &dir, &branch, &base);
        let SubWorktreeState::Recyclable { detail } = state else {
            panic!("a registration pointing at a deleted dir is recyclable, got {state:?}");
        };
        assert!(detail.contains("prunable"), "{detail}");
    }

    /// The branch checked out in ANOTHER live worktree. Reaping it would delete a
    /// directory that is not ours — refuse, and name what holds it.
    #[test]
    fn classify_refuses_a_branch_held_by_another_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-held");
        let dir = sub_worktree_path(&repo, "cls-held", "impl-1", 1);
        let branch = sub_worktree_branch("cls-held", "impl-1", 1);
        // Somebody checked our branch out somewhere else entirely.
        let elsewhere = repo.join("borrowed");
        create_worktree(&repo, &elsewhere, &branch, &base).unwrap();

        let state = classify_sub_worktree(&repo, &dir, &branch, &base);
        let SubWorktreeState::Occupied { detail } = state else {
            panic!("expected Occupied, got {state:?}");
        };
        assert!(detail.contains("borrowed"), "{detail}");
    }

    /// A non-empty directory git never registered. `worktree add` fails 128 there,
    /// and we have no idea what is in it → refuse.
    #[test]
    fn classify_refuses_a_foreign_non_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-foreign");
        let dir = sub_worktree_path(&repo, "cls-foreign", "impl-1", 1);
        let branch = sub_worktree_branch("cls-foreign", "impl-1", 1);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("junk"), "?\n").unwrap();

        assert!(matches!(
            classify_sub_worktree(&repo, &dir, &branch, &base),
            SubWorktreeState::Occupied { .. }
        ));
    }

    /// Measured nuance: an EMPTY unregistered directory is accepted by
    /// `git worktree add`, so `Absent` is not "nothing on disk".
    #[test]
    fn classify_treats_an_empty_foreign_directory_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-empty");
        let dir = sub_worktree_path(&repo, "cls-empty", "impl-1", 1);
        let branch = sub_worktree_branch("cls-empty", "impl-1", 1);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            classify_sub_worktree(&repo, &dir, &branch, &base),
            SubWorktreeState::Absent
        );
        // …and the create really does succeed from there.
        assert!(ensure_sub_worktree(&repo, &dir, &branch, &base, None).is_ok());
    }

    /// **THE REGRESSION.** `create_sub_worktree` on an existing sub-worktree fails
    /// 255; `ensure_sub_worktree` reuses it and touches nothing.
    #[test]
    fn ensure_reuses_an_existing_sub_worktree_and_destroys_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-reuse");
        let dir = sub_worktree_path(&repo, "ens-reuse", "impl-1", 1);
        let branch = sub_worktree_branch("ens-reuse", "impl-1", 1);

        let first = ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
        assert!(first.created);
        let original_base = first
            .base_sha
            .clone()
            .expect("a fresh cut records its base");

        // The bare create — what every re-spawn used to call — refuses outright.
        let err = create_sub_worktree(&repo, &dir, &branch, &base)
            .expect_err("git worktree add -b on an existing branch must fail");
        assert!(format!("{err:#}").contains("already exists"), "{err:#}");

        // Work in flight: an untracked file AND a tracked file modified. The tracked
        // leg is the load-bearing one — it is the only thing that distinguishes
        // "reused" from "committed first, then re-cut".
        std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();
        std::fs::write(dir.join("README.md"), "# edited\n").unwrap();
        let head_before = rev_parse(&dir, "HEAD").unwrap();

        let second =
            ensure_sub_worktree(&repo, &dir, &branch, &base, Some(&original_base)).unwrap();
        assert!(!second.created, "a reuse creates nothing");
        assert!(matches!(
            second.entry_state,
            SubWorktreeState::Reusable { has_work: true, .. }
        ));
        // #503 / ADR-0036: the ORIGINAL base, carried over. Re-deriving `HEAD` here
        // would yield the node's own commit and silently kill the adoption rule for
        // every restarted node.
        assert_eq!(second.base_sha.as_deref(), Some(original_base.as_str()));

        assert_eq!(
            std::fs::read_to_string(dir.join("scratch.txt")).unwrap(),
            "in flight\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("README.md")).unwrap(),
            "# edited\n"
        );
        assert_eq!(rev_parse(&dir, "HEAD").unwrap(), head_before);
    }

    /// The #498 lever: an orphaned branch is reaped and re-cut, and the reap now
    /// actually works (`worktree prune` before `branch -D`).
    #[test]
    fn ensure_recycles_an_orphaned_branch_and_re_cuts() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-recycle");
        let dir = sub_worktree_path(&repo, "ens-recycle", "impl-1", 1);
        let branch = sub_worktree_branch("ens-recycle", "impl-1", 1);
        ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
        // The failure mode #498 documents: the directory vanishes, the registration
        // and the branch ref stay.
        std::fs::remove_dir_all(&dir).unwrap();

        let again = ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
        assert!(again.created);
        assert!(matches!(
            again.entry_state,
            SubWorktreeState::Recyclable { .. }
        ));
        assert!(dir.exists());
        assert!(again.base_sha.is_some());
    }

    #[test]
    fn ensure_refuses_an_occupied_sub_worktree_without_touching_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-occupied");
        let dir = sub_worktree_path(&repo, "ens-occupied", "impl-1", 1);
        let branch = sub_worktree_branch("ens-occupied", "impl-1", 1);
        let elsewhere = repo.join("borrowed");
        create_worktree(&repo, &elsewhere, &branch, &base).unwrap();

        let err = ensure_sub_worktree(&repo, &dir, &branch, &base, None)
            .expect_err("an occupied sub-worktree must be refused, never reaped");
        assert!(format!("{err:#}").contains("occupied"), "{err:#}");
        // Untouched: the other worktree is still there, on the branch.
        assert!(elsewhere.exists());
        assert!(elsewhere.join("README.md").exists());
    }

    /// #489-B(a): the pre-#489 reap left BOTH locks in place when the directory had
    /// already gone — `branch -D` fails on a branch a registration still pins.
    #[test]
    fn reap_clears_a_registration_whose_directory_already_vanished() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "reap-missing");
        let dir = sub_worktree_path(&repo, "reap-missing", "impl-1", 1);
        let branch = sub_worktree_branch("reap-missing", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        reap_orphan_sub_worktree(&repo, &dir, &branch);

        let branches = std::process::Command::new("git")
            .args(["branch", "--list", &branch])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
            "the branch must be gone, not pinned by a stale registration"
        );
        // And the whole point: a fresh cut is possible again.
        assert!(create_sub_worktree(&repo, &dir, &branch, &base).is_ok());
    }

    /// #489-B(b) — **the silent total loss**. With a leftover `index.lock`,
    /// `git add -A` exits 128, `diff --cached --quiet` exits 0, no commit is taken,
    /// `git merge` says "Already up to date" and the pre-#489 code returned
    /// `MergeResult::Success` on 100% of the work lost — no conflict, no event, no
    /// trace.
    #[test]
    fn a_failing_git_add_fails_loudly_instead_of_reporting_a_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, base) = repo_with_pipeline_branch(&tmp, "add-fails");
        let wt_dir = repo.join(".pdo/runs/add-fails/worktree");
        let dir = sub_worktree_path(&repo, "add-fails", "impl-1", 1);
        let branch = sub_worktree_branch("add-fails", "impl-1", 1);
        create_sub_worktree(&repo, &dir, &branch, &base).unwrap();

        std::fs::write(dir.join("the_whole_point.rs"), "fn main() {}\n").unwrap();
        let gitdir = private_gitdir(&dir).unwrap();
        std::fs::write(gitdir.join("index.lock"), "").unwrap();

        let err = commit_and_merge_sub_worktree(&dir, &wt_dir, &branch, "impl-1", 1)
            .expect_err("a failed `git add -A` must not project to MergeResult::Success");
        assert!(
            format!("{err:#}").contains("git add -A"),
            "the failure must name what broke; got {err:#}"
        );
        // And the loss is what it protects: the file never reached the pipeline tree.
        assert!(!wt_dir.join("the_whole_point.rs").exists());
    }

    #[test]
    fn create_sub_worktree_spawn_failure_preserves_os_cause() {
        // Regression guard (#298): on the spawn-failure branch the OS-level
        // io::Error must survive as a *queryable* `source()`, i.e. a 2-link anyhow
        // chain [context, io::Error]. The pre-#298 duplicate used
        // `map_err(anyhow!("…: {e}"))`, which flattened the cause into the message
        // → a 1-link chain. Asserting the chain length (not the OS-specific string)
        // makes this test portable and gives it teeth against a regression to the
        // flattening form.
        let tmp = tempfile::tempdir().unwrap();

        // repo_root does NOT exist → git's chdir(current_dir) fails with ENOENT
        // *before* exec, so `.output()` returns Err(io::Error) (the spawn-failure
        // branch). This holds even if `git` is not installed on the runner.
        let nonexistent_repo = tmp.path().join("no-such-repo");

        // sub_worktree_dir's parent IS a valid writable temp path, so the earlier
        // `create_dir_all(parent)?` succeeds and does not shield the .output() error.
        let sub_wt_dir = tmp.path().join("sub").join("iter-1");

        let err = create_sub_worktree(&nonexistent_repo, &sub_wt_dir, "pdo/sub-x", "base")
            .expect_err("spawn must fail when repo_root does not exist");

        assert_eq!(
            err.chain().count(),
            2,
            "OS cause must be preserved as a distinct source(); got chain: {err:#}"
        );
    }

    // ---- #465 (ADR-0042): secondary read-only snapshots ----

    /// Seed a repo with a named tracked file and return its HEAD sha.
    fn init_repo_with_file(dir: &std::path::Path, name: &str, contents: &str) -> String {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join(name), contents).unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-m", "init"]);
        rev_parse(dir, "HEAD").unwrap()
    }

    #[test]
    fn secondary_snapshot_path_is_third_sibling() {
        let path = secondary_snapshot_path(
            std::path::Path::new("/primary"),
            "20260101-120000-abc",
            "repoB",
        );
        assert_eq!(
            path,
            PathBuf::from("/primary/.pdo/runs/20260101-120000-abc/repos/repoB"),
        );
    }

    #[test]
    fn rev_parse_verified_resolves_head_and_rejects_bogus() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let sha = init_repo_with_file(repo, "B.txt", "SECONDARY-MARKER-v1\n");
        assert_eq!(rev_parse_verified(repo, "HEAD").unwrap(), sha);
        // A ref that resolves to nothing / is ambiguous must fail loud.
        assert!(rev_parse_verified(repo, "no-such-branch").is_err());
    }

    #[test]
    fn create_secondary_snapshot_pins_and_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("repoA");
        let secondary = tmp.path().join("repoB");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        init_test_repo(&primary);
        let sha_b = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

        let run_id = "test-sec-run";
        let dest = secondary_snapshot_path(&primary, run_id, "repoB");
        create_secondary_snapshot(&secondary, &dest, &sha_b).unwrap();

        // The snapshot exists, is a THIRD sibling of worktree/ and nodes/, is pinned
        // to the frozen SHA, and its file is readable.
        assert!(dest.join("B.txt").exists());
        assert_eq!(rev_parse(&dest, "HEAD").unwrap(), sha_b);
        assert_eq!(
            std::fs::read_to_string(dest.join("B.txt")).unwrap(),
            "SECONDARY-MARKER-v1\n",
        );
        // Registered in the SECONDARY's .git, not the primary's.
        let listed = std::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&secondary)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&listed.stdout).contains("repos/repoB"));
    }

    #[test]
    fn secondary_snapshot_isolation_survives_local_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("repoA");
        let secondary = tmp.path().join("repoB");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        init_test_repo(&primary);
        let sha_v1 = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

        let dest = secondary_snapshot_path(&primary, "iso-run", "repoB");
        create_secondary_snapshot(&secondary, &dest, &sha_v1).unwrap();

        // Advance the operator's LOCAL checkout of the secondary AFTER the snapshot.
        std::fs::write(secondary.join("B.txt"), "SECONDARY-MARKER-v2\n").unwrap();
        let commit = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&secondary)
                .output()
                .unwrap();
        };
        commit(&["commit", "-aqm", "B: v2"]);

        // The snapshot must still be pinned at v1 (it is a snapshot, not a live mount).
        assert_eq!(rev_parse(&dest, "HEAD").unwrap(), sha_v1);
        assert_eq!(
            std::fs::read_to_string(dest.join("B.txt")).unwrap(),
            "SECONDARY-MARKER-v1\n",
        );
    }

    #[test]
    fn dirty_tracked_trips_guard_untracked_is_tolerated() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("repoA");
        let secondary = tmp.path().join("repoB");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        init_test_repo(&primary);
        let sha = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

        let dest = secondary_snapshot_path(&primary, "guard-run", "repoB");
        create_secondary_snapshot(&secondary, &dest, &sha).unwrap();

        // Clean snapshot → not dirty.
        assert!(!worktree_has_tracked_changes(&dest).unwrap());
        // Untracked scratch → tolerated (the probe ignores `??`).
        std::fs::write(dest.join("scratch_untracked.tmp"), "x").unwrap();
        assert!(!worktree_has_tracked_changes(&dest).unwrap());
        // A TRACKED modification → dirty (the guard trips).
        std::fs::write(dest.join("B.txt"), "tampered\n").unwrap();
        assert!(worktree_has_tracked_changes(&dest).unwrap());
    }

    #[test]
    fn remove_secondary_snapshot_prunes_registration_anti_498() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("repoA");
        let secondary = tmp.path().join("repoB");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        init_test_repo(&primary);
        let sha = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

        let dest = secondary_snapshot_path(&primary, "cleanup-run", "repoB");
        create_secondary_snapshot(&secondary, &dest, &sha).unwrap();

        remove_secondary_snapshot(&secondary, &dest);

        // The registration is gone from the secondary (prune ran), and a fresh
        // `worktree add` at the SAME path succeeds — the #498 anti-dangling control.
        let listed = std::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&secondary)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&listed.stdout).contains("repos/repoB"));

        let re_add = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&dest)
            .arg(&sha)
            .current_dir(&secondary)
            .output()
            .unwrap();
        assert!(
            re_add.status.success(),
            "re-add at the same path must succeed (no dangling registration): {}",
            String::from_utf8_lossy(&re_add.stderr)
        );
    }
}
