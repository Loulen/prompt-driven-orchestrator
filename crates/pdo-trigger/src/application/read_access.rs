//! A Trigger is in the Projets of its target repository, like a Run in those
//! of its primary repository: every Operation on it names them, and so does
//! every read. A Trigger without a target repository is in none.
//!
//! - A gesture on one Trigger: [`Triggers::on_trigger`] places its Operation.
//! - One Trigger, its fires: [`Triggers::authorize_read`], `403` on a refusal.
//! - The list: [`Triggers::readable`] keeps only the Triggers the actor may
//!   read, asking once per Trigger but working out the Projets once per
//!   repository.
//!
//! Without extension nothing is asked (`allows_everything`): the reads are
//! served exactly as before, and no Projet is looked up.

use std::collections::HashMap;

use pdo_kernel::authorization::authorize_read;
use pdo_kernel::{Actor, Operation, Verb};

use super::{operation, TriggerError};
use crate::domain::trigger::Trigger;
use crate::Triggers;

/// A repository a Trigger names: a field that is not blank, trimmed.
fn named_repo(field: Option<&str>) -> Option<&str> {
    field.map(str::trim).filter(|repo| !repo.is_empty())
}

impl Triggers {
    /// The Projets of the target repository `repo`; none without one.
    /// Without extension nobody reads them: none, and the Projets are not
    /// asked.
    pub(crate) async fn projects_of_repo(&self, repo: Option<&str>) -> Vec<String> {
        if self.extensions().allows_everything() {
            return Vec::new();
        }
        match named_repo(repo) {
            Some(repo) => self.projects.of_repository(repo).await,
            None => Vec::new(),
        }
    }

    /// `verb` on the Trigger `id`, in the Projets of the Trigger as stored.
    /// An unknown Trigger (or one the store cannot read) is in none: the
    /// gesture says why later, once allowed.
    pub(crate) async fn on_trigger(&self, verb: Verb, id: &str) -> Operation {
        if self.extensions().allows_everything() {
            return operation(verb, Some(id));
        }
        let repo = match self.store.get(id).await {
            Ok(Some(trigger)) => trigger.target_repo,
            _ => None,
        };
        operation(verb, Some(id)).in_projects(self.projects_of_repo(repo.as_deref()).await)
    }

    /// Whether `actor` may read the Trigger `id`: its detail, its fires, what
    /// `/ws` says of it.
    pub async fn authorize_read(&self, actor: &Actor, id: &str) -> Result<(), TriggerError> {
        if self.extensions().allows_everything() {
            return Ok(());
        }
        let read = self.on_trigger(Verb::Read, id).await;
        authorize_read(self.extensions(), actor, &read)?;
        Ok(())
    }

    /// What `actor` may read of a list of Triggers.
    pub fn readable<'a>(&'a self, actor: &'a Actor) -> ReadableTriggers<'a> {
        ReadableTriggers {
            triggers: self,
            actor,
            projects: HashMap::new(),
        }
    }
}

/// The filter of a list of Triggers for one actor, with the Projets of each
/// repository it met.
pub struct ReadableTriggers<'a> {
    triggers: &'a Triggers,
    actor: &'a Actor,
    projects: HashMap<String, Vec<String>>,
}

impl ReadableTriggers<'_> {
    /// Whether the actor may read `trigger`.
    pub async fn keeps(&mut self, trigger: &Trigger) -> bool {
        let host = self.triggers.extensions();
        if host.allows_everything() {
            return true;
        }
        let repo = named_repo(trigger.target_repo.as_deref()).map(str::to_string);
        let projects = match repo {
            None => Vec::new(),
            Some(repo) => match self.projects.get(&repo) {
                Some(projects) => projects.clone(),
                None => {
                    let projects = self.triggers.projects.of_repository(&repo).await;
                    self.projects.insert(repo, projects.clone());
                    projects
                }
            },
        };
        let read = operation(Verb::Read, Some(&trigger.id)).in_projects(projects);
        authorize_read(host, self.actor, &read).is_ok()
    }
}
