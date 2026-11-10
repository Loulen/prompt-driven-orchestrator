//! The Operations the gestures on a Run ask the Autorisation d'opération
//! about (ADR-0081): a node's is targeted `<run>/<node>`, a Run's by its id.
//! The scheduler's own transitions never build one (D10).
//!
//! Both are in the Projet recorded on the Run at its launch (ADR-0087), or in
//! none ([`Runs::projects_of_run`]).

use pdo_kernel::{Operation, ResourceKind, Verb};
use tracing::warn;

use crate::application::create_run::project::named_project;
use crate::application::create_run::CreateRunRequest;
use crate::application::session_claim::SessionClaim;
use crate::Runs;

/// `action` on the node `node_id` of `run_id`.
pub fn on_node(action: &str, run_id: &str, node_id: &str) -> Operation {
    Operation::new(ResourceKind::Node, Verb::Action(action.to_string()))
        .on(format!("{run_id}/{node_id}"))
}

/// `action` on the Run `run_id`.
pub fn on_run(action: &str, run_id: &str) -> Operation {
    Operation::new(ResourceKind::Run, Verb::Action(action.to_string())).on(run_id)
}

/// The Run an Operation on a Run or a node is about: its target (`<run>`, or
/// `<run>/<node>`). `None` for a creation and any other resource.
fn run_of(operation: &Operation) -> Option<&str> {
    let target = operation.target.as_deref()?;
    match operation.resource {
        ResourceKind::Run => Some(target),
        ResourceKind::Node => target.split_once('/').map(|(run, _)| run),
        _ => None,
    }
}

impl Runs {
    /// The Projets the Run `run_id` is in: the one recorded at its launch
    /// (ADR-0087), read from its Résumé (ADR-0084), never its log. None for
    /// an unknown Run, a Run launched in no Projet, or when the Résumé cannot
    /// be read (logged).
    pub async fn projects_of_run(&self, run_id: &str) -> Vec<String> {
        match self.store.summary(run_id).await {
            Ok(Some(summary)) => summary.project_id.into_iter().collect(),
            Ok(None) => Vec::new(),
            Err(e) => {
                warn!("the Projets of run {run_id} cannot be read: {e:#}");
                Vec::new()
            }
        }
    }

    /// The Projets a new Run would be in: for a Run created from a node
    /// session, the one of that session's Run, which it inherits (ADR-0064,
    /// ADR-0087); else the one the request names. A claim the creation
    /// refuses later never creates anything.
    pub async fn projects_of_new_run(
        &self,
        req: &CreateRunRequest,
        session: Option<&SessionClaim>,
    ) -> Vec<String> {
        match (session, named_project(req)) {
            (Some(claim), _) => self.projects_of_run(&claim.run_id).await,
            (None, Some(project)) => vec![project.to_string()],
            (None, None) => Vec::new(),
        }
    }

    /// `operation`, in the Projets of the Run its target names, unless it
    /// already names its Projets.
    pub async fn scoped(&self, operation: Operation) -> Operation {
        if !operation.is_outside_projects() {
            return operation;
        }
        match run_of(&operation) {
            Some(run_id) => {
                let projects = self.projects_of_run(run_id).await;
                operation.in_projects(projects)
            }
            None => operation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_or_node_operation_names_its_run_by_its_target() {
        assert_eq!(run_of(&on_run("rename_run", "r1")), Some("r1"));
        assert_eq!(run_of(&on_node("retry", "r1", "n-1")), Some("r1"));
        assert_eq!(
            run_of(&Operation::new(ResourceKind::Run, Verb::Create)),
            None
        );
        assert_eq!(
            run_of(&Operation::new(ResourceKind::Trigger, Verb::Delete).on("t1")),
            None
        );
    }
}
