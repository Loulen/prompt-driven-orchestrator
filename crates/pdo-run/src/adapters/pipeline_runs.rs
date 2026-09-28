//! The port of the pipeline context the Runs answer: how many Runs still use a
//! pipeline, which a rename or a delete refuses to orphan. (The Stats
//! absorption a rename records is the stats context's `StatsRenames`, #928.)

use std::sync::Arc;

use pdo_pipeline::application::ports::{ActiveRuns, BoxFuture};

use crate::application::ports::EventStore;
use crate::domain::projection::project;
use crate::domain::state::RunStatus;

/// The Runs of the event log.
pub struct PipelineRuns(pub Arc<dyn EventStore>);

impl ActiveRuns for PipelineRuns {
    /// A third "active run" predicate, deliberately distinct from
    /// `RunStatus::is_terminal()`, which also treats Failed/Skipped/Halted as
    /// non-active: rename and delete would orphan the completion path of any
    /// Run not yet completed nor archived (its fallback resolves the pipeline
    /// by stem), so they refuse.
    fn using<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let mut active_count: usize = 0;
            if let Ok(run_ids) = self.0.run_ids().await {
                for run_id in &run_ids {
                    let Ok(events) = self.0.events(run_id).await else {
                        continue;
                    };
                    let Some(run_state) = project(&events) else {
                        continue;
                    };
                    if run_state.pipeline_name == pipeline_id
                        && run_state.status != RunStatus::Completed
                        && run_state.status != RunStatus::Archived
                    {
                        active_count += 1;
                    }
                }
            }
            active_count
        })
    }
}
