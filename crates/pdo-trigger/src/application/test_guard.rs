//! Dry-run a guard command (`POST /triggers/guard/test`): the opposite pole of
//! "Run now" (ADR-0027 addendum). `trigger` × `test_guard`: the dry-run runs an
//! arbitrary `sh -c` in a repository, so an extension may refuse it.
//!
//! Zero-side-effect invariant: it resolves the repository, reads the timeout and
//! runs the guard — nothing else. It never fires, never touches the store (no
//! fire row, no `next_fire_at` recompute) and never starts a Run.

use pdo_kernel::{Actor, Verb};
use serde::Deserialize;

use super::{operation, TriggerError, TEST_GUARD};
use crate::domain::fire_decision::GuardResult;
use crate::Triggers;

/// The guard command *as currently typed in the New-trigger tab* plus an
/// optional target repo. No trigger id — this is pre-save.
#[derive(Debug, Clone, Deserialize)]
pub struct GuardTestRequest {
    pub guard_command: String,
    #[serde(default)]
    pub target_repo: Option<String>,
}

impl Triggers {
    pub async fn test_guard(
        &self,
        actor: &Actor,
        req: GuardTestRequest,
    ) -> Result<GuardResult, TriggerError> {
        self.authorize(
            actor,
            &operation(Verb::Action(TEST_GUARD.to_string()), None),
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
        // Same `stored → env → default` precedence a real fire uses (ADR-0015), so the
        // dry-run reflects the bound the scheduler would apply.
        let timeout = self.settings.guard_timeout().await;
        Ok(self
            .guards
            .run(req.guard_command.trim(), &cwd, timeout)
            .await)
    }
}
