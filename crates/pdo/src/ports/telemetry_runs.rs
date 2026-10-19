//! The Runs as the telemetry context sees them: what a Run's measures are
//! attached to (its Projet, its repository named by its remote, git, its
//! pipeline and harness), its cost and tokens once it ended, and the live
//! sessions. Never the Actor: nothing here reads who launched or answered.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::project_names::ProjectNames;
use pdo_run::adapters::disk_live_facts::DiskLiveFacts;
use pdo_run::application::events::reload_run_state_with;
use pdo_run::domain::event::EventKind;
use pdo_run::domain::run_cost::RunCostBreakdown;
use pdo_run::Runs;
use pdo_session::Session;
use pdo_telemetry::adapters::git;
use pdo_telemetry::application::ports::{BoxFuture, LiveSessions, RunCosts, RunFactsSource};
use pdo_telemetry::domain::attributes::RunFacts;
use pdo_telemetry::domain::cost::{CostSlice, Tokens};
use pdo_telemetry::domain::lifecycle::RunLaunch;
use pdo_telemetry::domain::repository::repository_label;

/// The run context and the Projets' names behind the telemetry's facts.
pub(crate) struct RunFactsOfRuns {
    pub(crate) runs: Arc<Runs>,
    pub(crate) project_names: Arc<ProjectNames>,
    /// The repository of a historical Run that names none (ADR-0033).
    pub(crate) repo_root: PathBuf,
}

impl RunFactsSource for RunFactsOfRuns {
    fn facts<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, RunFacts> {
        Box::pin(async move {
            let events = pdo_run::application::events::load_events(&self.runs, run_id)
                .await
                .unwrap_or_default();
            let launch = RunLaunch::from_payload(
                events
                    .iter()
                    .find(|e| e.kind == EventKind::RunStarted)
                    .and_then(|e| e.payload.as_ref()),
            );
            let project = self
                .runs
                .projects_of_run(run_id)
                .await
                .into_iter()
                .next()
                .map(|id| self.project_names.name(&id).unwrap_or(id));
            let folder = launch
                .repository
                .clone()
                .unwrap_or_else(|| self.repo_root.clone());
            let source_branch = launch.source_branch.clone();
            let (remote, branch) = tokio::task::spawn_blocking(move || {
                let branch = source_branch.or_else(|| git::current_branch(&folder));
                (git::remote_url(&folder), branch, folder)
            })
            .await
            .map(|(remote, branch, folder)| (repository_label(remote.as_deref(), &folder), branch))
            .unwrap_or_default();
            RunFacts {
                project,
                repository: Some(remote).filter(|r| !r.is_empty()),
                branch,
                commit: launch.fork_sha,
                pipeline: launch.pipeline,
                harness: launch.harness,
            }
        })
    }
}

/// The cost of an ended Run, read off the harnesses' files.
pub(crate) struct RunCostsOfRuns {
    pub(crate) runs: Arc<Runs>,
    pub(crate) disk: Arc<DiskLiveFacts>,
}

impl RunCosts for RunCostsOfRuns {
    fn slices<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<CostSlice>> {
        Box::pin(async move {
            let Some((events, run)) = reload_run_state_with(&self.runs, run_id).await else {
                return Vec::new();
            };
            let disk = self.disk.clone();
            tokio::task::spawn_blocking(move || slices_of(disk.cost_breakdown(&run, &events)))
                .await
                .unwrap_or_default()
        })
    }
}

/// One slice per model of every execution; an execution whose source names
/// no model is one slice of its harness.
fn slices_of(breakdown: RunCostBreakdown) -> Vec<CostSlice> {
    breakdown
        .contributions
        .into_iter()
        .flat_map(|contribution| {
            if contribution.model_slices.is_empty() {
                return vec![CostSlice {
                    harness: contribution.harness,
                    model: None,
                    usd: contribution.usd,
                    tokens: None,
                }];
            }
            let harness = contribution.harness;
            contribution
                .model_slices
                .into_iter()
                .map(|slice| CostSlice {
                    harness: harness.clone(),
                    model: Some(slice.model),
                    usd: slice.usd,
                    tokens: slice.tokens.map(|t| Tokens {
                        input: t.input,
                        output: t.output,
                        cache_read: t.cache_read,
                        cache_write: t.cache_write,
                    }),
                })
                .collect()
        })
        .collect()
}

/// The live node sessions of the session context.
pub(crate) struct SessionsOfTelemetry(pub(crate) Arc<Session>);

impl LiveSessions for SessionsOfTelemetry {
    fn live(&self) -> BoxFuture<'_, Option<i64>> {
        Box::pin(async move { Some(self.0.read_status().execute().await.live as i64) })
    }
}
