//! The delivery of a NodeRun's work onto the Run's branch (#654, ADR-0060).

use anyhow::{Context, Result};
use tracing::info;

use super::*;

/// Stage node-authored work and commit it under
/// [`delivery_commit_message`], or answer `Ok(None)` when nothing was left to
/// commit (#654 / ADR-0060).
///
/// Staging starts with `git add -A`, then removes paths PDO materialized through
/// declarative worktree provisioning (ADR-0061) and PDO's own artifact store
/// [`RUN_ARTIFACTS_RELATIVE`] (#796). This second step is required even when a
/// Node already staged those paths itself: provisioning commonly carries
/// credentials and host-local links that must never enter the Run branch, and
/// committed artifacts travel to the next run of the same pipeline (a child run
/// created from a branch that carries them), where the same node ids put a
/// previous execution's `Pass` report exactly where the fresh node will write.
/// The target repo's `.gitignore` remains the policy for every other path. A node
/// that already committed its own work reaches this with a clean index and gets
/// `None` — its commits are kept as they are, and no empty commit is ever written.
///
/// #489: the exit STATUS of `git add -A` is checked, not just the spawn. Discarding
/// it loses the whole node's work in silence, and reusing a sub-worktree (#489-B)
/// promotes that from latent to routine. The measured chain, with a leftover
/// `index.lock`:
///
/// ```text
/// git add -A                  -> exit 128  "Unable to create '…/index.lock': File exists"
/// git diff --cached --quiet   -> exit 0    => NO COMMIT TAKEN
/// git merge pdo/sub-…         -> "Already up to date."  exit 0
/// => MergeResult::Success — the agent's file is ABSENT from the pipeline worktree
/// ```
///
/// `pdo complete` answered `Success`, the Run went green, and 100% of the
/// uncommitted work vanished with no conflict, no event and no trace — the silent
/// loss ADR-0004 forbids. `bail!` here surfaces as
/// `CompletionRefusal::DeliveryFailed`, which is loud and parks the Run.
///
/// `identity` signs the commit (CORE-43); an empty one leaves git to the
/// daemon's own environment and configuration.
pub fn stage_and_commit(
    worktree_dir: &std::path::Path,
    node_id: &str,
    iter: i64,
    identity: &CommitIdentity,
) -> Result<Option<String>> {
    let add_output = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree_dir)
        .output()
        .with_context(|| format!("git add -A failed to run in {}", worktree_dir.display()))?;
    if !add_output.status.success() {
        anyhow::bail!(
            "git add -A in {} failed, refusing to report a delivery that would drop the node's \
             work: {}",
            worktree_dir.display(),
            git_report(&add_output)
        );
    }

    // Paths PDO itself put in the worktree, never the node's deliverable: the
    // artifact store first (#796), then whatever provisioning materialized.
    let mut excluded: Vec<Vec<u8>> = vec![RUN_ARTIFACTS_RELATIVE.as_bytes().to_vec()];
    excluded.extend(crate::adapters::git::manifest::materialized_paths(
        worktree_dir,
    )?);
    {
        let mut child = std::process::Command::new("git")
            .args([
                "reset",
                "--quiet",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .current_dir(worktree_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "git reset of PDO-owned paths failed to run in {}",
                    worktree_dir.display()
                )
            })?;
        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .context("git reset stdin unavailable")?;
            for path in excluded {
                stdin.write_all(b":(top,literal)")?;
                stdin.write_all(&path)?;
                stdin.write_all(&[0])?;
            }
        }
        let output = child
            .wait_with_output()
            .context("wait for git reset of PDO-owned paths")?;
        if !output.status.success() {
            anyhow::bail!(
                "git reset of PDO-owned paths in {} failed, refusing to commit provisioned \
                 resources or run artifacts: {}",
                worktree_dir.display(),
                git_report(&output)
            );
        }
    }

    let status_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(worktree_dir)
        .output()
        .context("git diff --cached failed")?;

    // Exit 0 ⇒ the index matches HEAD ⇒ nothing left to deliver. No empty commit.
    if status_output.status.success() {
        return Ok(None);
    }

    let commit_msg = delivery_commit_message(node_id, iter);
    let output = std::process::Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .envs(identity.vars())
        .current_dir(worktree_dir)
        .output()
        .with_context(|| format!("git commit failed to run in {}", worktree_dir.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "git commit in {} failed: {}",
            worktree_dir.display(),
            git_report(&output)
        );
    }

    Ok(Some(rev_parse(worktree_dir, "HEAD")?))
}

/// What a delivery did to the Run's branch (#654 / ADR-0060).
///
/// `before == after` is the **no-op**: the NodeRun left nothing git could see, so
/// the Run's branch did not move and no commit was written. Every other case is a
/// delivery whose content is exactly `git diff before after` — which is what makes
/// a per-NodeRun diff answerable without knowing whether the node was isolated.
#[derive(Debug, Clone)]
pub struct Delivered {
    /// The Run branch's tip before the delivery.
    pub before: String,
    /// The Run branch's tip after it.
    pub after: String,
}

impl Delivered {
    /// Did the Run's branch actually move?
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

/// The outcome of the single delivery operation every completion path goes through
/// (#654 / ADR-0060).
///
/// The conflict arms only ever come from an isolated NodeRun: a non-isolated one
/// commits in place and has nothing to merge.
#[derive(Debug)]
pub enum DeliveryOutcome {
    /// The work is on the Run's branch (possibly a no-op — see [`Delivered`]).
    Delivered(Delivered),
    /// The merge-back conflicted and was resolved in the node's favour (#503).
    ResolvedInNodeFavour(MergeAdoption),
    Conflict(MergeConflict),
}

/// **The** delivery: keep the NodeRun's own commits, commit whatever it left
/// behind, and put the result on the Run's branch (#654 / ADR-0060).
///
/// One operation for both isolations, because the difference is a *where*, not a
/// *what*: an isolated NodeRun commits in its sub-worktree and its branch is then
/// merged into the Run's; a non-isolated one commits directly in the Run's
/// worktree. Both keep any commit the node made itself, both refuse to write an
/// empty commit, and both stage under the repo's own `.gitignore` while excluding
/// paths PDO materialized through declarative worktree provisioning.
///
/// No serialisation is added for the shared worktree beyond what git itself needs:
/// two non-isolated NodeRuns may run concurrently, and the first one here commits
/// every non-ignored change then visible — under its own deterministic message.
/// Isolation, not the runtime, is how an author buys reliable attribution.
///
/// `spawn_base`: the commit the NodeRun's work started from, per its `NodeStarted`.
/// For an isolated NodeRun, the commit its sub-worktree was cut from
/// (`merge_action::spawn_base_sha`): `None` — a pre-#503 Run, or a spawn that
/// recorded none — forbids the #503 resolution, an unknown base is not a licence
/// to rewrite a branch. For a non-isolated one, the Run worktree's HEAD when it
/// started (`merge_action::start_head_sha`): it is the delivery's `before`, so the
/// commits the node made itself stay inside its pair (CORE-231). `None` there
/// falls back to the HEAD at delivery, which only brackets what was left behind.
///
/// `identity`: the identity frozen on the NodeRun's start (CORE-43). It signs
/// every commit this delivery writes: the delivery commit, the merge-back and
/// the #503 resolution. Empty, git keeps the daemon's own.
#[allow(clippy::too_many_arguments)]
pub fn deliver_node_work(
    run_worktree_dir: &std::path::Path,
    node_worktree_dir: &std::path::Path,
    isolated: bool,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
    spawn_base: Option<&str>,
    identity: &CommitIdentity,
) -> Result<DeliveryOutcome> {
    let before = rev_parse(run_worktree_dir, "HEAD")?;

    if !isolated {
        // In place: the node worked in the Run's worktree, so its commit IS the
        // delivery. Nothing to merge, nothing to conflict with. The pair starts
        // where the node started, not where it stopped: a node that committed its
        // own work would otherwise deliver an empty pair.
        let before = spawn_base.map(str::to_string).unwrap_or(before);
        stage_and_commit(run_worktree_dir, node_id, iter, identity)?;
        let after = rev_parse(run_worktree_dir, "HEAD")?;
        if before == after {
            info!("Node {node_id} iter-{iter} left nothing to deliver in the run worktree");
        } else {
            info!("Delivered {node_id} iter-{iter} in place: {before} -> {after}");
        }
        return Ok(DeliveryOutcome::Delivered(Delivered { before, after }));
    }

    let merge = commit_and_merge_sub_worktree_inner(
        node_worktree_dir,
        run_worktree_dir,
        sub_branch,
        node_id,
        iter,
        spawn_base,
        identity,
    )?;

    Ok(match merge {
        MergeResult::Success => DeliveryOutcome::Delivered(Delivered {
            after: rev_parse(run_worktree_dir, "HEAD")?,
            before,
        }),
        MergeResult::ResolvedInNodeFavour(adoption) => {
            DeliveryOutcome::ResolvedInNodeFavour(adoption)
        }
        MergeResult::Conflict(c) => DeliveryOutcome::Conflict(c),
    })
}
