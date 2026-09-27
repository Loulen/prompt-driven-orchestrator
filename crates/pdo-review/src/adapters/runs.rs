//! The run context behind [`ReviewRuns`]: its log, its projection, its
//! repository and worktree, its Pipeline Manager started on demand and the
//! manager's tmux session.

use std::sync::Arc;
use std::time::Duration;

use pdo_run::application::events::{append_event, load_events};
use pdo_run::application::manager::ensure_run_manager;
use pdo_run::application::paths::effective_repo_root;
use pdo_run::application::refs::run_worktree_dir;
use pdo_run::domain::event::Event;
use pdo_run::domain::projection::project;
use pdo_run::domain::state::RunState;
use pdo_run::Runs;

use crate::application::ports::{BoxFuture, ReviewRuns, ReviewedRun};

/// How long a manager spawned by the send waits before the batch is pasted —
/// the harness has to be reading its input, or the paste lands in a shell.
const PASTE_DELAY_AFTER_SPAWN: Duration = Duration::from_secs(8);
/// The grace before a paste into a manager that was already running.
const PASTE_DELAY: Duration = Duration::from_millis(300);

/// The Runs of the run context, as the review reads and writes them.
pub struct ContextRuns(pub Arc<Runs>);

impl ReviewRuns for ContextRuns {
    fn load<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<Option<ReviewedRun>, String>> {
        Box::pin(async move {
            let events = load_events(&self.0, run_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(project(&events).map(|state| ReviewedRun {
                repo: effective_repo_root(&self.0, &state),
                worktree: run_worktree_dir(&self.0, &state, run_id),
                events,
                state,
            }))
        })
    }

    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            append_event(&self.0, event)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn ensure_manager<'a>(
        &'a self,
        run_id: &'a str,
        run: &'a RunState,
    ) -> BoxFuture<'a, Result<bool, (u16, serde_json::Value)>> {
        Box::pin(async move {
            ensure_run_manager(&self.0, run_id, run)
                .await
                .map_err(|(status, body)| (status.as_u16(), body))
        })
    }

    fn hand_to_manager(&self, run_id: &str, message: String, just_started: bool) {
        let runs = self.0.clone();
        let session = pdo_session::domain::names::manager_session_name(run_id);
        let delay = if just_started {
            PASTE_DELAY_AFTER_SPAWN
        } else {
            PASTE_DELAY
        };
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            pdo_session::application::messages::paste_message(runs.tmux(), &session, &message);
        });
    }
}
