//! Cut a node's sub-worktree, or make an existing one usable again (#489-B).

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::*;

/// Create a node's sub-worktree and return **the commit it was cut from**.
///
/// That SHA is the whole basis of the #503 adoption rule (ADR-0036): a
/// merge-back may be resolved in the node's favour only when the pipeline
/// branch's tip is *still* this commit. Returned rather than re-derived so the
/// two spawn paths (`node_spawn`, `node_primitives`) cannot record different
/// things — or forget to record it.
pub fn create_sub_worktree(
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

/// What [`ensure_sub_worktree`] settled on.
#[derive(Debug)]
pub struct EnsuredSubWorktree {
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
/// same iteration, i.e. on every `restart_node` of an isolated
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
pub fn ensure_sub_worktree(
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
