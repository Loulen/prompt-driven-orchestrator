//! The reads of the Runs go through the Autorisation d'opération (CORE-5): a
//! Run is read in the Projet of its primary repository, like every gesture on
//! it ([`Runs::scoped`]).
//!
//! - One Run (its detail, its nodes, its artifacts…): [`Runs::authorize_read`],
//!   `403` on a refusal.
//! - A list: [`Runs::readable`] keeps only the Runs the actor may read. It
//!   asks once per Run, but works out the Projet once per repository: a list
//!   of 500 Runs over a handful of repositories asks the store a handful of
//!   times.
//!
//! Without extension nothing is asked (`allows_everything`): the lists are
//! served exactly as before.

use std::collections::HashMap;

use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use crate::application::paths::effective_repo_root;
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
        ReadableRuns {
            runs: self,
            actor,
            projects: HashMap::new(),
        }
    }
}

/// The filter of a list of Runs for one actor, with the Projet of each
/// repository it met.
pub struct ReadableRuns<'a> {
    runs: &'a Runs,
    actor: &'a Actor,
    projects: HashMap<String, Vec<String>>,
}

impl ReadableRuns<'_> {
    /// Whether the actor may read `run`.
    pub async fn keeps(&mut self, run: &RunState) -> bool {
        let host = self.runs.extensions();
        if host.allows_everything() {
            return true;
        }
        let repo = effective_repo_root(self.runs, run)
            .to_string_lossy()
            .into_owned();
        let projects = match self.projects.get(&repo) {
            Some(projects) => projects.clone(),
            None => {
                let projects = self.runs.projects_of_repository(&repo).await;
                self.projects.insert(repo, projects.clone());
                projects
            }
        };
        let operation = read_run(&run.run_id).in_projects(projects);
        authorize_read(host, self.actor, &operation).is_ok()
    }

    /// `runs`, without those the actor may not read.
    pub async fn retain(&mut self, runs: &mut Vec<RunState>) {
        let mut kept = Vec::with_capacity(runs.len());
        for run in runs.drain(..) {
            if self.keeps(&run).await {
                kept.push(run);
            }
        }
        *runs = kept;
    }
}
