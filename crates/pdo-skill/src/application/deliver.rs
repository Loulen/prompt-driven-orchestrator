//! Deliver a NodeRun's skills actifs into the tree it works in (#672): the Run
//! snapshot extended (additively) with what is new, then the copy + link +
//! exclusion of each skill. A transition of the scheduler, not an actor's
//! gesture: no Autorisation d'opération (D10).

use std::path::Path;

use super::skills::Skills;
use crate::domain::delivery::{snapshot_root, SkippedSkill};
use crate::domain::selection::ActiveSkill;

/// What a delivery could not write, skill by skill. Never a failure: the node
/// runs without them and the Run view says so.
pub type Delivered = Vec<SkippedSkill>;

impl Skills {
    /// Freeze `skills` into the Run's snapshot, then deliver them into
    /// `worktree`. An I/O failure is an error: the spawn refuses, like
    /// provisioning.
    pub fn deliver(
        &self,
        worktree: &Path,
        run_repo_root: &Path,
        run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Delivered> {
        let mut skipped = self.delivery.snapshot(run_repo_root, run_id, skills)?;
        let mut report = self.delivery.deliver(
            worktree,
            &snapshot_root(run_repo_root, run_id),
            run_id,
            skills,
        )?;
        skipped.append(&mut report.skipped);
        Ok(skipped)
    }

    /// Take a Run's exclusions back from the repository's `info/exclude`
    /// (Run cleanup, or a Run aborted at create).
    pub fn withdraw(&self, repo_root: &Path, run_id: &str) -> anyhow::Result<()> {
        self.delivery.withdraw(repo_root, run_id)
    }
}
