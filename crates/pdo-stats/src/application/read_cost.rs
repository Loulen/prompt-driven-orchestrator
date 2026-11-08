//! `GET /stats/cost` — the lazy heavy read: the Runs started in the window,
//! each Run's memoized harness-specific contributions, folded into the
//! hierarchies. Fetched by the client only when the Cost tab is shown.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pdo_run::application::ports::RunStart;
use pdo_run::domain::prices::PriceTable;
use pdo_workspace::domain::project::Project;

use super::absorptions::load_resolver;
use super::ports::{AbsorptionStore, Projects, RunEvents, RunFiles};
use super::read_overview::{ReadError, StatsRead};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::bucket::strftime_fmt;
use crate::domain::cost::fold::{
    fold_harness_cost, project_identity, resolved_price_rows, CostRunRow,
};
use crate::domain::cost::roll_up::{fold_rolled_up_cost, RollUpCohort, RolledTree};
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
        Ok(self.fold(read).await?.cost)
    }

    /// « Roll up child runs » (ADR draft-cout-de-l-arbre §2): the cohort is
    /// recomposed by **root** — every root Run of the window (« completed runs
    /// only » judging the root) brings its whole tree, archived descendants
    /// included, each member in its root's period and Project. « By model »
    /// stays the fold of the unchanged cohort.
    pub async fn execute_rolled_up(&self, read: &StatsRead) -> Result<StatsCost, ReadError> {
        let folded = self.fold(read).await?;
        let links = self.events.child_run_starts().await.map_err(failed)?;
        let cohort = RollUpCohort::new(links.iter().filter_map(|start| {
            let parent = start.payload.get("parent_run_id")?.as_str()?;
            Some((start.run_id.as_str(), parent))
        }));
        let mut starts: HashMap<&str, &RunStart> = HashMap::new();
        for start in &links {
            starts.insert(start.run_id.as_str(), start);
        }

        let mut trees = Vec::new();
        for root in folded.rows.iter().filter(|row| cohort.is_root(&row.run_id)) {
            let mut members = Vec::new();
            for (run_id, depth) in cohort.descendants(&root.run_id) {
                let Some(start) = starts.get(run_id.as_str()) else {
                    continue;
                };
                let mut row = self.cost_row(start, &folded.context).await?;
                row.bucket = root.bucket.clone();
                row.project_id = root.project_id.clone();
                row.project_name = root.project_name.clone();
                members.push((depth, row));
            }
            trees.push(RolledTree {
                root: root.clone(),
                members,
            });
        }
        Ok(fold_rolled_up_cost(
            &trees,
            folded.cost,
            &folded.context.resolver,
        ))
    }

    /// The window's cohort, each Run costed, and its fold.
    async fn fold(&self, read: &StatsRead) -> Result<Folded, ReadError> {
        let Some(fmt) = strftime_fmt(&read.bucket) else {
            return Err(ReadError::InvalidBucket(read.bucket.clone()));
        };
        // #810: the memo this fold reads is per-Run (keyed by the Run's own event
        // fingerprint), so the cohort switch cannot stale it — it selects which
        // Runs enter the fold, never what one Run costs.
        let resolver = load_resolver(self.absorptions.as_ref(), read.uncombined)
            .await
            .map_err(failed)?;
        let starts = self
            .events
            .run_starts(Some(fmt), read.window())
            .await
            .map_err(failed)?;
        // #427: the three price tiers, resolved ONCE for the whole fold — never
        // inside the per-Run loop. The table's fingerprint is the memo's third
        // key component, so a sync is visible here without a daemon restart.
        let context = FoldContext {
            resolver,
            prices: self.files.prices(),
            projects: self.projects.list().await.map_err(failed)?,
        };

        let mut rows: Vec<CostRunRow> = Vec::with_capacity(starts.len());
        for start in &starts {
            rows.push(self.cost_row(start, &context).await?);
        }
        let cost = fold_harness_cost(
            &rows,
            resolved_price_rows(&context.prices),
            &context.resolver,
        );
        Ok(Folded {
            rows,
            cost,
            context,
        })
    }

    /// One Run, costed through the per-Run memo, under its own period,
    /// Pipeline and Project.
    async fn cost_row(
        &self,
        start: &RunStart,
        context: &FoldContext,
    ) -> Result<CostRunRow, ReadError> {
        // Pipeline key: `pipeline_id` going forward (#377), else the (always
        // present) `pipeline_name`, absorptions applied (#890).
        let identity = context.resolver.run_identity(&start.payload);
        let (project_id, project_name) =
            project_identity(&start.payload, &self.repo_root, &context.projects);
        // #408: the transcripts of a sandboxed Run are read from its staged
        // home while it lives; #708: `pi`'s store follows the same seam.
        let roots = self.files.roots(&start.run_id, &start.payload);
        let events = self.events.events(&start.run_id).await.map_err(failed)?;
        let contributions =
            self.files
                .cost_contributions(&events, &roots, &start.run_id, &context.prices);
        Ok(CostRunRow {
            run_id: start.run_id.clone(),
            started_at: start.ts.clone(),
            bucket: start.bucket.clone().unwrap_or_default(),
            identity,
            project_id,
            project_name,
            contributions,
        })
    }
}

/// A source failed: the read says which.
fn failed(e: impl std::fmt::Display) -> ReadError {
    ReadError::Failed(format!("stats cost failed: {e}"))
}

/// What every Run of one fold is read with — resolved once per request.
struct FoldContext {
    resolver: AbsorptionResolver,
    prices: PriceTable,
    projects: Vec<Project>,
}

struct Folded {
    rows: Vec<CostRunRow>,
    cost: StatsCost,
    context: FoldContext,
}
