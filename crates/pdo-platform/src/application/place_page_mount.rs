//! Where a page-mount Operation is judged: in the Projet of the Run it belongs
//! to. A mount asked from a node session is in the Projet of that session's
//! Run; an unmount, in the Projet of the Run the mount belongs to. A mount
//! outside any Run, or a Run launched in no Projet, stays in no Projet, under
//! the instance's own rules.
//!
//! The mount and the unmount place their Operation here before asking the
//! Autorisation d'opération; the question of authorization (`POST
//! /authorize`) does too, so it answers what the gesture would get.

use std::sync::Arc;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::ports::{PageMountRepository, Runs};

pub struct PlacePageMount {
    pages: Arc<dyn PageMountRepository>,
    runs: Arc<dyn Runs>,
}

impl PlacePageMount {
    pub fn new(pages: Arc<dyn PageMountRepository>, runs: Arc<dyn Runs>) -> Self {
        Self { pages, runs }
    }

    /// `operation`, asked by `actor`, in the Projets the gesture is in. An
    /// Operation that already names its Projets, or that is not about a page
    /// mount, is returned as is.
    pub async fn execute(&self, actor: &Actor, operation: Operation) -> Operation {
        if operation.resource != ResourceKind::PageMount || !operation.is_outside_projects() {
            return operation;
        }
        let run_id = match (&operation.verb, actor, operation.target.as_deref()) {
            (Verb::Create, Actor::Agent { run_id, .. }, _) => Some(run_id.clone()),
            (Verb::Delete, _, Some(name)) => match self.pages.get(name).await {
                Ok(mount) => mount.and_then(|mount| mount.run_id),
                Err(e) => {
                    tracing::warn!("the Run of page mount {name} cannot be read: {e}");
                    None
                }
            },
            _ => None,
        };
        match run_id {
            Some(run_id) => {
                let projects = self.runs.projects_of_run(&run_id).await;
                operation.in_projects(projects)
            }
            None => operation,
        }
    }
}
