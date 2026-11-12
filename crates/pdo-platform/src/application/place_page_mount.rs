//! Where a page-mount Operation is judged: in the Projet of the mount. A mount
//! asked from a node session is in the Projet of that session's Run; a read
//! or an unmount, in the Projet the mount is in. A mount outside any Run, or
//! a Run launched in no Projet, stays in no Projet, under the instance's own
//! rules, unless the mount names its Projet itself.
//!
//! The use cases place their Operation here before asking the Autorisation
//! d'opération; the question of authorization (`POST /authorize`) does too,
//! so it answers what the gesture would get.

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
        let projects = match (&operation.verb, actor, operation.target.as_deref()) {
            (Verb::Create, Actor::Agent { run_id, .. }, _) => self.project_of_run(run_id).await,
            (Verb::Delete | Verb::Read, _, Some(name)) => match self.pages.get(name).await {
                Ok(mount) => mount.and_then(|mount| mount.project_id),
                Err(e) => {
                    tracing::warn!("the Projet of page mount {name} cannot be read: {e}");
                    None
                }
            },
            _ => None,
        };
        operation.in_projects(projects)
    }

    /// Whether `operation` is the unmount of a page `actor` owns: the core
    /// allows it without asking the extensions, and so does the question of
    /// authorization.
    pub async fn owned_by(&self, actor: &Actor, operation: &Operation) -> bool {
        if operation.resource != ResourceKind::PageMount || operation.verb != Verb::Delete {
            return false;
        }
        let Some(name) = operation.target.as_deref() else {
            return false;
        };
        match self.pages.get(name).await {
            Ok(mount) => mount.is_some_and(|mount| mount.is_owned_by(actor)),
            Err(e) => {
                tracing::warn!("the owner of page mount {name} cannot be read: {e}");
                false
            }
        }
    }

    /// The Projet a mount made from a node session of `run_id` is in.
    pub async fn project_of_run(&self, run_id: &str) -> Option<String> {
        self.runs.projects_of_run(run_id).await.into_iter().next()
    }
}
