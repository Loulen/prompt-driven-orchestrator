//! The reads of the Runs go through the Autorisation d'opération (CORE-5): a
//! Run is read in the Projet recorded at its launch (ADR-0087), like every
//! gesture on it ([`Runs::scoped`]).
//!
//! - One Run (its detail, its nodes, its artifacts…): [`Runs::authorize_read`],
//!   `403` on a refusal.
//! - A list: [`Runs::readable`] keeps only the Runs the actor may read, asking
//!   once per Run; the Projet is on the Run itself, so no store is read.
//!
//! Without extension nothing is asked (`allows_everything`): the lists are
//! served exactly as before.

use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::domain::run_summary::RunSummary;
use crate::domain::state::RunState;
use crate::Runs;

/// Reading the Run `run_id`.
pub fn read_run(run_id: &str) -> Operation {
    Operation::new(ResourceKind::Run, Verb::Read).on(run_id)
}

impl Runs {
    /// Whether `actor` may read the Run `run_id`, in the Projets of the Run.
    pub async fn authorize_read(&self, actor: &Actor, run_id: &str) -> Result<(), Denied> {
        if self.extensions().allows_everything() {
            return Ok(());
        }
        let operation = self.scoped(read_run(run_id)).await;
        authorize_read(self.extensions(), actor, &operation)
    }

    /// What `actor` may read of a list of Runs.
    pub fn readable<'a>(&'a self, actor: &'a Actor) -> ReadableRuns<'a> {
        ReadableRuns { runs: self, actor }
    }
}

/// The filter of a list of Runs for one actor.
pub struct ReadableRuns<'a> {
    runs: &'a Runs,
    actor: &'a Actor,
}

impl ReadableRuns<'_> {
    /// Whether the actor may read `run`.
    pub async fn keeps(&mut self, run: &impl ListedRun) -> bool {
        let host = self.runs.extensions();
        if host.allows_everything() {
            return true;
        }
        let operation = read_run(run.run_id()).in_projects(run.project_id());
        authorize_read(host, self.actor, &operation).is_ok()
    }

    /// `runs`, without those the actor may not read.
    pub async fn retain<R: ListedRun>(&mut self, runs: &mut Vec<R>) {
        let mut kept = Vec::with_capacity(runs.len());
        for run in runs.drain(..) {
            if self.keeps(&run).await {
                kept.push(run);
            }
        }
        *runs = kept;
    }
}

/// A Run of a list, as the read filter reads it: a projected state or a
/// Résumé de Run (ADR-0084).
pub trait ListedRun {
    fn run_id(&self) -> &str;
    /// The Projet recorded on the Run (ADR-0087).
    fn project_id(&self) -> Option<&str>;
}

impl ListedRun for RunState {
    fn run_id(&self) -> &str {
        &self.run_id
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
}

impl ListedRun for RunSummary {
    fn run_id(&self) -> &str {
        &self.run_id
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
}
