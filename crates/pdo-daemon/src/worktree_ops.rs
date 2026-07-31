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
/// `NodeStarted` (#279). The worktree was created at the pipeline branch's tip
/// with no agent run, so removing it loses no work. Best-effort throughout
/// (mirrors `cleanup_run`): a missing dir / branch is fine.
pub(crate) fn reap_orphan_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
) {
    if sub_worktree_dir.exists() {
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(sub_worktree_dir)
            .current_dir(repo_root)
            .output();
    }
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", sub_branch])
        .current_dir(repo_root)
        .output();
    info!(
        "Reaped orphaned sub-worktree {} (branch {sub_branch}) after aborted spawn (#279)",
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
    let _ = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git add failed in sub-worktree")?;

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
}
