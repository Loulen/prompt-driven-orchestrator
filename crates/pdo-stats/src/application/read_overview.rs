//! `GET /stats/overview` — the cheap section: indexed counts per period over
//! the event log and the Triggers' fires, then the Sessions fold over the Node
//! starts of the Runs started in the window.

use std::collections::BTreeSet;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;
use pdo_run::application::ports::EventWindow;

use super::absorptions::load_resolver;
use super::ports::{AbsorptionStore, RunEvents, TriggerFires};
use super::readable::{readable_runs, readable_triggers};
use crate::domain::bucket::strftime_fmt;
use crate::domain::overview::fires::fires_by_pipeline;
use crate::domain::overview::sessions::session_stats;
use crate::domain::overview::{assemble, BucketCount, OverviewSeries, StatsOverview};

/// One read of a Stats section: the window, its bucket, the cohort switch and
/// the absorptions switch the three sections share.
#[derive(Debug, Clone)]
pub struct StatsRead {
    /// Inclusive lower bound (ISO-8601, e.g. `2026-07-15T00:00:00Z`).
    pub from: String,
    /// Exclusive upper bound.
    pub to: String,
    /// `day` | `week` | `month`.
    pub bucket: String,
    /// Only the Runs that reached `RunCompleted` (#810).
    pub completed_only: bool,
    /// No absorption applied (#888, ADR-0077).
    pub uncombined: bool,
}

impl StatsRead {
    /// The window of the read, over the Runs of `runs` only when given (the
    /// ones its reader may read).
    pub fn window<'a>(&'a self, runs: Option<&'a BTreeSet<String>>) -> EventWindow<'a> {
        EventWindow {
            from: &self.from,
            to: &self.to,
            completed_only: self.completed_only,
            runs,
        }
    }
}

/// Why a section could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The bucket is none of `day`, `week`, `month`.
    InvalidBucket(String),
    /// A source failed; the message names it.
    Failed(String),
}

fn buckets(rows: Vec<(String, i64)>) -> Vec<BucketCount> {
    rows.into_iter()
        .map(|(bucket, count)| BucketCount { bucket, count })
        .collect()
}

pub struct ReadOverview {
    events: Arc<dyn RunEvents>,
    triggers: Arc<dyn TriggerFires>,
    absorptions: Arc<dyn AbsorptionStore>,
    extensions: Arc<dyn ExtensionHost>,
}

impl ReadOverview {
    pub fn new(
        events: Arc<dyn RunEvents>,
        triggers: Arc<dyn TriggerFires>,
        absorptions: Arc<dyn AbsorptionStore>,
        extensions: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            events,
            triggers,
            absorptions,
            extensions,
        }
    }

    /// The overview `actor` reads: the Runs and the Triggers it may read.
    pub async fn execute(
        &self,
        actor: &Actor,
        read: &StatsRead,
    ) -> Result<StatsOverview, ReadError> {
        let Some(fmt) = strftime_fmt(&read.bucket) else {
            return Err(ReadError::InvalidBucket(read.bucket.clone()));
        };
        let failed = |e: anyhow::Error| ReadError::Failed(format!("stats overview failed: {e}"));
        let resolver = load_resolver(self.absorptions.as_ref(), read.uncombined)
            .await
            .map_err(failed)?;
        let host = self.extensions.as_ref();
        let runs = readable_runs(host, self.events.as_ref(), actor)
            .await
            .map_err(failed)?;
        let triggers = readable_triggers(host, self.triggers.as_ref(), actor)
            .await
            .map_err(failed)?;
        let window = read.window(runs.as_ref());
        // `run_skipped` is NOT an error (invariant #4): errors = `run_failed` only.
        let runs = self.events.count_by_period("run_started", fmt, window);
        let runs = buckets(runs.await.map_err(failed)?);
        let errors = self.events.count_by_period("run_failed", fmt, window);
        let errors = buckets(errors.await.map_err(failed)?);
        // Sessions = `node_started` starts (re-spawns and loop laps included,
        // manager excluded by construction): the periods of the x-axis. The
        // series itself is the cohort's fold below.
        let node_starts = self.events.count_by_period("node_started", fmt, window);
        let node_starts = buckets(node_starts.await.map_err(failed)?);
        let fires = self
            .triggers
            .by_pipeline(&read.from, &read.to, triggers.as_ref());
        let fires = fires_by_pipeline(fires.await.map_err(failed)?, &resolver);
        let created = self
            .triggers
            .created_runs(&read.from, &read.to, triggers.as_ref());
        let created = created.await.map_err(failed)?;
        let rows = self.events.node_starts(fmt, window).await.map_err(failed)?;
        let sessions = session_stats(rows, &resolver);
        Ok(assemble(
            OverviewSeries {
                runs,
                errors,
                fires_by_pipeline: fires,
                triggers_created_runs: created,
                sessions,
            },
            &node_starts,
        ))
    }
}
