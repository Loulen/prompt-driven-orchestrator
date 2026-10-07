//! The Operations the gestures on a Run ask the Autorisation d'opération
//! about (ADR-0081): a node's is targeted `<run>/<node>`, a Run's by its id.
//! The scheduler's own transitions never build one (D10).
//!
//! Both are in the Projet of the Run's primary repository (CORE-3): the one
//! that owns it, or none ([`Runs::projects_of_run`]).

use pdo_kernel::repository_path::member_spellings;
use pdo_kernel::{Operation, ResourceKind, Verb};
use tracing::warn;

use crate::application::create_run::CreateRunRequest;
use crate::application::paths::effective_repo_of;
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

/// A repository a request names: a field that is not blank, trimmed.
fn named_repo(field: Option<&str>) -> Option<&str> {
    field.map(str::trim).filter(|repo| !repo.is_empty())
}

impl Runs {
    /// The Projets the Run `run_id` is in: the one that owns its primary
    /// repository, read from its Résumé (ADR-0084), never its log. None for
    /// an unknown Run, for a repository no Projet owns, or when the Projets
    /// cannot be read (logged).
    pub async fn projects_of_run(&self, run_id: &str) -> Vec<String> {
        let summary = match self.store.summary(run_id).await {
            Ok(Some(summary)) => summary,
            Ok(None) => return Vec::new(),
            Err(e) => {
                warn!("the Projets of run {run_id} cannot be read: {e:#}");
                return Vec::new();
            }
        };
        let repo = effective_repo_of(self, summary.target_repo.as_deref());
        self.projects_of_repository(&repo.to_string_lossy()).await
    }

    /// The Projets the repository `path` is in: the one that owns it, or none.
    ///
    /// A Projet's members are compared verbatim (ADR-0033), but a Run works in
    /// whatever directory its path names: `/repo/`, `/repo/.` or a link to
    /// `/repo` must not take a Run out of the Projet owning `/repo`. So when
    /// the path itself is no member, its other spellings are tried: without
    /// trailing `/` nor `.`, then resolved on disk.
    pub async fn projects_of_repository(&self, path: &str) -> Vec<String> {
        for spelling in member_spellings(path) {
            match self.projects.owner_of(&spelling).await {
                Ok(Some(owner)) => return vec![owner.id],
                Ok(None) => {}
                Err(e) => {
                    warn!("the Projet owning {path} cannot be read: {e}");
                    return Vec::new();
                }
            }
        }
        Vec::new()
    }

    /// The Projets a new Run would be in: those of the primary repository the
    /// request names (`target_repo`, else `target_repos[0]`), else, for a Run
    /// created from a node session, those of that session's Run, whose
    /// repository it then inherits (ADR-0064). A claim the creation refuses
    /// later never creates anything.
    pub async fn projects_of_new_run(
        &self,
        req: &CreateRunRequest,
        session: Option<&SessionClaim>,
    ) -> Vec<String> {
        let named = named_repo(req.target_repo.as_deref())
            .or_else(|| named_repo(req.target_repos.first().map(|r| r.repo.as_str())));
        match (named, session) {
            (Some(repo), _) => self.projects_of_repository(repo).await,
            (None, Some(claim)) => self.projects_of_run(&claim.run_id).await,
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
