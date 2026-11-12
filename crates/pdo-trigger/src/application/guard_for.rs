//! Running a guard for someone (CONTEXT.md → *Trigger*): the environment the
//! extensions give it for that Actor in that Projet, the guard itself, and its
//! end told to the extensions, so they revoke what they handed it. Without an
//! extension, the guard runs exactly as before: nothing added.

use std::path::Path;

use pdo_kernel::extension_host::{ExtensionHost, GitOperation, GuardEnd, GuardOutcome, GuardRun};
use pdo_kernel::Actor;

use crate::domain::fire_decision::GuardResult;
use crate::Triggers;

/// Who and where a guard runs for, and what it runs.
pub(crate) struct GuardFor<'a> {
    /// The last editor of the Trigger on a fire, who asks on a dry-run.
    pub actor: &'a Actor,
    /// The Projet of the Trigger.
    pub project: Option<&'a str>,
    /// The Trigger, `None` on the dry-run of an unsaved guard.
    pub trigger_id: Option<&'a str>,
    pub command: &'a str,
    pub cwd: &'a Path,
}

impl Triggers {
    /// Run `guard.command` with the environment the extensions give
    /// `guard.actor` in `guard.project`, then tell them it ended, whatever
    /// its end.
    pub(crate) async fn run_guard_for(&self, guard: GuardFor<'_>) -> GuardResult {
        let run = GuardRun::new(
            uuid::Uuid::new_v4().to_string(),
            guard.trigger_id.map(str::to_string),
        );
        let mut operation =
            GitOperation::guard(guard.actor.clone(), guard.cwd.display().to_string(), run);
        if let Some(project) = guard.project {
            operation = operation.in_project(project);
        }
        let credentials = self.extensions().git_credentials(&operation);
        // From here on, the extensions hear the end whatever happens: a
        // dropped request (its guard killed with it) ends it as failed.
        let ending = Ending {
            host: self.extensions(),
            operation: Some(operation),
        };
        // Resolve the timeout fresh each time so a settings change takes
        // effect without a restart.
        let timeout = self.settings.guard_timeout().await;
        let result = self
            .guards
            .run(guard.command, guard.cwd, &credentials.env, timeout)
            .await;
        ending.end(outcome(&result));
        result
    }
}

/// The end of a guard, owed to the extensions once they were asked its
/// environment: told by [`Ending::end`], else as failed when dropped.
struct Ending<'a> {
    host: &'a dyn ExtensionHost,
    operation: Option<GitOperation>,
}

impl Ending<'_> {
    fn end(mut self, outcome: GuardOutcome) {
        self.tell(outcome);
    }

    fn tell(&mut self, outcome: GuardOutcome) {
        if let Some(end) = self
            .operation
            .take()
            .and_then(|operation| GuardEnd::of(&operation, outcome))
        {
            self.host.guard_ended(&end);
        }
    }
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        self.tell(GuardOutcome::Failed);
    }
}

/// How a guard's result reads to the extensions.
fn outcome(result: &GuardResult) -> GuardOutcome {
    match result {
        GuardResult::Pass { .. } => GuardOutcome::Passed,
        GuardResult::Skip { .. } => GuardOutcome::Skipped,
        GuardResult::Error {
            timed_out: true, ..
        } => GuardOutcome::TimedOut,
        GuardResult::Error { .. } => GuardOutcome::Failed,
    }
}
