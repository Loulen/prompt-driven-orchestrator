//! `GET /stats/cost` — the lazy heavy read: the Runs started in the window,
//! each Run's memoized harness-specific contributions, folded into the
//! hierarchies. Fetched by the client only when the Cost tab is shown.

use std::path::PathBuf;
use std::sync::Arc;

use super::absorptions::load_resolver;
use super::ports::{AbsorptionStore, Projects, RunEvents, RunFiles};
use super::read_overview::{ReadError, StatsRead};
use crate::domain::bucket::strftime_fmt;
use crate::domain::cost::fold::{
    fold_harness_cost, project_identity, resolved_price_rows, CostRunRow,
};
use crate::domain::cost::StatsCost;

pub struct ReadCost {
    events: Arc<dyn RunEvents>,
    absorptions: Arc<dyn AbsorptionStore>,
    projects: Arc<dyn Projects>,
    files: Arc<dyn RunFiles>,
    /// The daemon's repo root: the Projet of a Run that names no repository.
    repo_root: PathBuf,
}

impl ReadCost {
    pub fn new(
        events: Arc<dyn RunEvents>,
        absorptions: Arc<dyn AbsorptionStore>,
        projects: Arc<dyn Projects>,
        files: Arc<dyn RunFiles>,
        repo_root: PathBuf,
    ) -> Self {
        Self {
            events,
            absorptions,
            projects,
            files,
            repo_root,
        }
    }

    pub async fn execute(&self, read: &StatsRead) -> Result<StatsCost, ReadError> {
        let Some(fmt) = strftime_fmt(&read.bucket) else {
            return Err(ReadError::InvalidBucket(read.bucket.clone()));
        };
        let failed = |e: String| ReadError::Failed(format!("stats cost failed: {e}"));
        // #810: the memo this fold reads is per-Run (keyed by the Run's own event
        // fingerprint), so the cohort switch cannot stale it — it selects which
        // Runs enter the fold, never what one Run costs.
        let resolver = load_resolver(self.absorptions.as_ref(), read.uncombined)
            .await
            .map_err(|e| failed(e.to_string()))?;
        let rows = self
            .events
            .run_starts(Some(fmt), read.window())
            .await
            .map_err(|e| failed(e.to_string()))?;
        // #427: the three price tiers, resolved ONCE for the whole fold — never
        // inside the per-Run loop. The table's fingerprint is the memo's third
        // key component, so a sync is visible here without a daemon restart.
        let prices = self.files.prices();
        let stored_projects = self.projects.list().await.map_err(failed)?;

        let mut cost_rows: Vec<CostRunRow> = Vec::with_capacity(rows.len());
        for row in rows {
            // Pipeline key: `pipeline_id` going forward (#377), else the (always
            // present) `pipeline_name`, absorptions applied (#890).
            let identity = resolver.run_identity(&row.payload);
            let (project_id, project_name) =
                project_identity(&row.payload, &self.repo_root, &stored_projects);
            // #408: the transcripts of a sandboxed Run are read from its staged
            // home while it lives; #708: `pi`'s store follows the same seam.
            let roots = self.files.roots(&row.run_id, &row.payload);
            let events = self
                .events
                .events(&row.run_id)
                .await
                .map_err(|e| failed(e.to_string()))?;
            let contributions =
                self.files
                    .cost_contributions(&events, &roots, &row.run_id, &prices);
            cost_rows.push(CostRunRow {
                run_id: row.run_id,
                started_at: row.ts,
                bucket: row.bucket.unwrap_or_default(),
                identity,
                project_id,
                project_name,
                contributions,
            });
        }

        Ok(fold_harness_cost(
            &cost_rows,
            resolved_price_rows(&prices),
            &resolver,
        ))
    }
}
