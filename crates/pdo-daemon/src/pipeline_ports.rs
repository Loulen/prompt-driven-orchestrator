//! The port of the pipeline context that the Runs implement, while they still
//! live in this crate: how many Runs use a pipeline. (The Stats absorption a
//! rename records is the stats context's `StatsRenames`, #928.)

use pdo_pipeline::application::ports::{ActiveRuns, BoxFuture};

use crate::{event_log, load_all_run_ids, load_events};

/// The Runs of the daemon's database.
pub(crate) struct LegacyActiveRuns {
    pub(crate) db: sqlx::SqlitePool,
}

impl ActiveRuns for LegacyActiveRuns {
    /// A third "active run" predicate, deliberately distinct from
    /// `RunStatus::is_terminal()`, which also treats Failed/Skipped/Halted as
    /// non-active: rename and delete would orphan the completion path of any
    /// Run not yet completed nor archived (its fallback resolves the pipeline
    /// by stem), so they refuse.
    fn using<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let mut active_count: usize = 0;
            if let Ok(run_ids) = load_all_run_ids(&self.db).await {
                for run_id in &run_ids {
                    let Ok(events) = load_events(&self.db, run_id).await else {
                        continue;
                    };
                    let Some(run_state) = event_log::project(&events) else {
                        continue;
                    };
                    if run_state.pipeline_name == pipeline_id
                        && run_state.status != event_log::RunStatus::Completed
                        && run_state.status != event_log::RunStatus::Archived
                    {
                        active_count += 1;
                    }
                }
            }
            active_count
        })
    }
}
