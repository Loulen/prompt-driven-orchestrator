//! Deliver a NodeRun's skills actifs into the tree it works in (#672): the Run
//! snapshot extended (additively) with what is new, then the copy + link +
//! exclusion of each skill. A transition of the scheduler, not an actor's
//! gesture: no Autorisation d'opération (D10). And the skills PDO manages
//! delivered into a folder outside any Run (the Agent de support's).

use std::path::Path;

use super::skills::Skills;
use crate::domain::delivery::{snapshot_root, DeliveryReport, SkippedSkill};
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

    /// Deliver the skills PDO manages, its own and its extensions' (the
    /// locked ones of the bank), into `folder`, a folder PDO owns outside any
    /// repository: an agent working outside any Run (the Agent de support)
    /// learns PDO and the extensions from them. What an earlier delivery left
    /// there is replaced. An I/O failure is an error.
    pub async fn deliver_managed(&self, folder: &Path) -> anyhow::Result<DeliveryReport> {
        let managed: Vec<ActiveSkill> = self
            .skills
            .list()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .into_iter()
            .filter(|skill| skill.locked)
            .map(|skill| ActiveSkill {
                id: skill.id,
                name: skill.name,
                tiers: Vec::new(),
            })
            .collect();
        self.delivery.deliver_to_folder(folder, &managed)
    }
}
