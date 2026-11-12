//! A Trigger is in the Projet it launches its Runs in (ADR-0087), like a Run
//! in the one recorded at its launch: every Operation on it names it, and so
//! does every read. A Trigger launching in no Projet is in none.
//!
//! - A gesture on one Trigger: [`Triggers::on_trigger`] places its Operation;
//!   [`Triggers::scoped`] places a probe of it the same way.
//! - One Trigger, its fires: [`Triggers::authorize_read`], `403` on a refusal,
//!   the answer about an unknown Trigger on a concealed one.
//! - The list: [`Triggers::readable`] keeps only the Triggers the actor may
//!   read, asking once per Trigger.
//!
//! Without extension nothing is asked (`allows_everything`): the reads are
//! served exactly as before.

use pdo_kernel::authorization::{authorize_entity_read, authorize_read, EntityRead};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::{operation, TriggerError};
use crate::domain::trigger::Trigger;
use crate::Triggers;

/// A Projet a request names: a field that is not blank, trimmed.
pub(crate) fn named_project(field: Option<&str>) -> Option<String> {
    field
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// The Projets of a Trigger launching in `project`: it, or none.
pub(crate) fn projects_of(project: Option<&str>) -> Vec<String> {
    project.into_iter().map(str::to_string).collect()
}

impl Triggers {
    /// `verb` on the Trigger `id`, in the Projets of the Trigger as stored.
    pub(crate) async fn on_trigger(&self, verb: Verb, id: &str) -> Operation {
        if self.extensions().allows_everything() {
            return operation(verb, Some(id));
        }
        operation(verb, Some(id)).in_projects(self.projects_of_trigger(id).await)
    }

    /// `operation`, placed in the Projets of the Trigger it targets when it
    /// names none, as the gesture itself would be: so the question of
    /// authorization (`POST /authorize`) judges a probe like the gesture.
    /// Any other Operation, or one naming its Projets, is left as asked.
    pub async fn scoped(&self, operation: Operation) -> Operation {
        if operation.resource != ResourceKind::Trigger || !operation.is_outside_projects() {
            return operation;
        }
        let Some(id) = operation.target.clone() else {
            return operation;
        };
        let projects = self.projects_of_trigger(&id).await;
        operation.in_projects(projects)
    }

    /// The Projets of the Trigger `id` as stored. An unknown Trigger (or one
    /// the store cannot read) is in none: the gesture says why later, once
    /// allowed.
    async fn projects_of_trigger(&self, id: &str) -> Vec<String> {
        let project = match self.store.get(id).await {
            Ok(Some(trigger)) => trigger.project_id,
            _ => None,
        };
        projects_of(project.as_deref())
    }

    /// Whether `actor` may read the Trigger `id`: its detail, its fires, what
    /// `/ws` says of it. [`EntityRead::AsUnknown`] on a concealed refusal:
    /// the read answers as for a Trigger that does not exist.
    pub async fn authorize_read(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<EntityRead, TriggerError> {
        if self.extensions().allows_everything() {
            return Ok(EntityRead::Shown);
        }
        let read = self.on_trigger(Verb::Read, id).await;
        Ok(authorize_entity_read(self.extensions(), actor, &read)?)
    }

    /// What `actor` may read of a list of Triggers.
    pub fn readable<'a>(&'a self, actor: &'a Actor) -> ReadableTriggers<'a> {
        ReadableTriggers {
            triggers: self,
            actor,
        }
    }
}

/// The filter of a list of Triggers for one actor.
pub struct ReadableTriggers<'a> {
    triggers: &'a Triggers,
    actor: &'a Actor,
}

impl ReadableTriggers<'_> {
    /// Whether the actor may read `trigger`.
    pub async fn keeps(&mut self, trigger: &Trigger) -> bool {
        let host = self.triggers.extensions();
        if host.allows_everything() {
            return true;
        }
        let projects = projects_of(trigger.project_id.as_deref());
        let read = operation(Verb::Read, Some(&trigger.id)).in_projects(projects);
        authorize_read(host, self.actor, &read).is_ok()
    }
}
