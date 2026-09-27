//! Merge a node's sub-worktree back into the Run's branch, and the #503
//! resolution in the node's favour (ADR-0036).

use std::path::Path;

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::*;

/// Everything a conflicting merge-back knows about itself (#503, AC3/AC4).
///
/// Exists because `MergeResult::Conflict(String)` could not answer the only two
/// questions a post-mortem asks — *which files?* and *which two commits?* — and
/// the one string it did carry was empty (see [`MergeConflict::detail`]).
#[derive(Debug, Clone)]
pub struct MergeConflict {
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
pub const ADOPTION_RULE: &str = "the pipeline branch's tip is still the commit \
     this node's sub-worktree was cut from, so the divergence is the run's own history \
     rewritten by the node";

/// A conflict resolved in the node's favour instead of failing the run (#503).
#[derive(Debug, Clone)]
pub struct MergeAdoption {
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
pub enum MergeResult {
    Success,
    /// #503: the three-way merge conflicted, but resolving it entirely in the
    /// node's favour provably drops nothing, so the pipeline branch was moved onto
    /// a merge commit carrying the node's tree. Not a failure — and never silent.
    ResolvedInNodeFavour(MergeAdoption),
    Conflict(MergeConflict),
    ConflictPendingResolution(MergeConflict),
}

/// `spawn_base`: the commit this sub-worktree was cut from, per its `NodeStarted`
/// (`merge_action::spawn_base_sha`). `None` — a pre-#503 Run, or a spawn that
/// recorded none — forbids the #503 resolution: an unknown base is not a licence to
/// rewrite a branch.
pub fn commit_and_merge_sub_worktree_inner(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
    keep_conflict: bool,
    spawn_base: Option<&str>,
) -> Result<MergeResult> {
    // The node's own commits are kept; only what it left uncommitted becomes a
    // commit here, and nothing at all when it left nothing (#654).
    stage_and_commit(sub_worktree_dir, node_id, iter)?;

    // Both tips, read BEFORE the merge mutates either worktree (#503 AC4): a
    // post-mortem that has to rediscover these two SHAs by hand is the
    // archaeology this issue is about.
    let pipeline_tip = rev_parse(pipeline_worktree_dir, "HEAD")?;
    let node_tip = rev_parse(sub_worktree_dir, "HEAD")?;

    // #599 / ADR-0036 (amended): merge the sub-worktree's **real HEAD**
    // (`node_tip`), NOT the `pdo/sub-*` branch name. When the agent switched its
    // worktree onto a `feature/…` branch (a project git-flow — ADR-0050 §3), its
    // commits live on `feature/…` while `pdo/sub-*` stayed behind at the cut base;
    // merging by name brought that stale tip back ("Already up to date") and
    // dropped the node's work in silence. `node_tip` is what the adoption/conflict
    // logic already keys on, so this keeps the two consistent. On the ordinary case
    // (agent stayed on `pdo/sub-*`) `node_tip` is that branch's tip, so this is
    // byte-for-byte the old behaviour.
    let output = std::process::Command::new("git")
        .args(["merge", &node_tip, "--no-edit"])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("git merge failed")?;

    if !output.status.success() {
        let conflict = MergeConflict {
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
pub fn rev_parse(dir: &std::path::Path, rev: &str) -> Result<String> {
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
pub fn rev_parse_verified(dir: &Path, rev: &str) -> Result<String> {
    let arg = format!("{rev}^{{commit}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &arg])
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git rev-parse --verify {arg}"))?;
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
pub fn git_report(output: &std::process::Output) -> String {
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
pub fn unmerged_paths(worktree_dir: &std::path::Path) -> Vec<String> {
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
///   in silence. It can legitimately be dirty: a non-isolated node in flight
///   (#654 — it works there and PDO delivers what it leaves), the Run shell, the
///   resident `__manager__` agent.
/// - unrelated histories: `git merge` also fails (`refusing to merge unrelated
///   histories`) when the two tips share no ancestor. That is not a diverged
///   pipeline branch, and adopting would replace the run's whole tree.
///
/// `false` means the conflict is genuine as far as PDO can tell, and AC2 says it
/// stays a failure.
pub fn adoption_allowed(
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

pub fn share_an_ancestor(repo_dir: &std::path::Path, a: &str, b: &str) -> bool {
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
pub fn resolve_in_node_favour(
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
