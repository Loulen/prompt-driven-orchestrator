//! Dry-run a guard command (`POST /triggers/guard/test`): the opposite pole of
//! "Run now" (ADR-0027 addendum). `trigger` × `test_guard`: the dry-run runs an
//! arbitrary `sh -c` in a repository, so an extension may refuse it.
//!
//! Zero-side-effect invariant: it resolves the repository, reads the timeout and
//! runs the guard with the environment the extensions give who asks it (told
//! when it ends, as on a fire) — nothing else. It never fires, never touches the store (no
//! fire row, no `next_fire_at` recompute) and never starts a Run.

use pdo_kernel::{Actor, Verb};
use serde::Deserialize;

use super::guard_for::GuardFor;
use super::read_access::{named_project, projects_of};
use super::{operation, TriggerError, TEST_GUARD};
use crate::domain::fire_decision::GuardResult;
use crate::Triggers;

/// The guard command *as currently typed in the New-trigger tab* plus an
/// optional target repo and the Projet the Trigger is in, if any. No trigger
/// id — this is pre-save.
#[derive(Debug, Clone, Deserialize)]
pub struct GuardTestRequest {
    pub guard_command: String,
    #[serde(default)]
    pub target_repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

impl Triggers {
    pub async fn test_guard(
        &self,
        actor: &Actor,
        req: GuardTestRequest,
    ) -> Result<GuardResult, TriggerError> {
        // Judged in the Projet it names: the guard runs with what the extensions
        // give the caller there.
        let project = named_project(req.project_id.as_deref());
        self.authorize(
            actor,
            &operation(Verb::Action(TEST_GUARD.to_string()), None)
                .in_projects(projects_of(project.as_deref())),
        )?;
        if req.guard_command.trim().is_empty() {
            return Err(TriggerError::Invalid(
                "guard_command must not be empty".to_string(),
            ));
        }
        // Same rule as a real fire: a guard must never execute in a repository nobody
        // named. The dry-run is pre-save, but it runs the same arbitrary `sh -c`.
        let cwd = self
            .repositories
            .required(req.target_repo.as_deref())
            .map_err(TriggerError::Invalid)?;
        // Same `stored → env → default` timeout and same environment a real fire
        // uses (ADR-0015), so the dry-run reflects what the scheduler would do; it
        // acts for who asks it, in the Projet the Trigger is (or will be) in.
        Ok(self
            .run_guard_for(GuardFor {
                actor,
                project: project.as_deref(),
                trigger_id: None,
                command: req.guard_command.trim(),
                cwd: &cwd,
            })
            .await)
    }
}
