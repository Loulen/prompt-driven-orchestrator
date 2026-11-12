use serde::Serialize;

use crate::{Actor, NodeEnv};

/// What a network git operation of the daemon does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GitOperationKind {
    /// Add a repository by its URL: its full clone, or the check that a clone
    /// already on disk answers to the Actor before it is reused.
    Clone,
    /// Fetch every remote of a repository (New Run, its synchronisation).
    Fetch,
    /// The shallow clone of a Source de skills, to scan or import it.
    Skills,
    /// The fetch before a Trigger fires, for its last editor.
    TriggerFetch,
    /// The guard of a Trigger, the shell command run before it fires (and its
    /// dry-run): `url` is the path of its target repository (the
    /// `PDO_TARGET_REPO` of the guard), the operation's `guard` names the
    /// execution. The variables reach the guard's shell, never its argv;
    /// [`Extension::guard_ended`](crate::Extension::guard_ended) says when it
    /// is over.
    Guard,
}

/// A network git operation the daemon runs for an Actor, as
/// [`Extension::git_env`](crate::Extension::git_env) sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct GitOperation {
    /// Who the operation is for: the Actor of the gesture, the last editor of
    /// the Trigger for [`GitOperationKind::TriggerFetch`] (the Trigger itself
    /// when no one edited it since the core started recording editors).
    pub actor: Actor,
    /// The Projet the operation is in, if any.
    pub project: Option<String>,
    /// The remote's URL, as git is given it; for a
    /// [`GitOperationKind::Guard`], the path of the target repository.
    pub url: String,
    pub kind: GitOperationKind,
    /// The guard's execution, for a [`GitOperationKind::Guard`] only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<GuardRun>,
}

impl GitOperation {
    /// `kind` on `url` for `actor`, in no Projet.
    pub fn new(actor: Actor, url: impl Into<String>, kind: GitOperationKind) -> Self {
        Self {
            actor,
            project: None,
            url: url.into(),
            kind,
            guard: None,
        }
    }

    /// The guard `guard`, run in `target_repo` for `actor`, in no Projet.
    pub fn guard(actor: Actor, target_repo: impl Into<String>, guard: GuardRun) -> Self {
        Self {
            guard: Some(guard),
            ..Self::new(actor, target_repo, GitOperationKind::Guard)
        }
    }

    /// The same operation, in the Projet `project`.
    pub fn in_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }
}

/// One execution of a guard: its id, unique to it, which
/// [`GuardEnd`] repeats, and the Trigger it guards (`None` for the dry-run of
/// a guard not saved yet).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub struct GuardRun {
    pub id: String,
    pub trigger_id: Option<String>,
}

impl GuardRun {
    /// The execution `id` of the guard of the Trigger `trigger_id`, if any.
    pub fn new(id: impl Into<String>, trigger_id: Option<String>) -> Self {
        Self {
            id: id.into(),
            trigger_id,
        }
    }
}

/// How a guard ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GuardOutcome {
    /// It exited 0: the Trigger fires.
    Passed,
    /// It exited non-zero (or a signal killed it): the Trigger skips.
    Skipped,
    /// It could not run (its shell did not start, its process failed).
    Failed,
    /// It ran past its timeout and was killed.
    TimedOut,
}

/// The end of a guard, as
/// [`Extension::guard_ended`](crate::Extension::guard_ended) sees it: the
/// execution, who and where it ran for (as [`GitOperation`] gave them), how it
/// ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct GuardEnd {
    pub guard: GuardRun,
    pub actor: Actor,
    pub project: Option<String>,
    pub outcome: GuardOutcome,
}

impl GuardEnd {
    /// The end of the guard `operation` describes, with `outcome`; `None`
    /// when `operation` is no guard.
    pub fn of(operation: &GitOperation, outcome: GuardOutcome) -> Option<Self> {
        Some(Self {
            guard: operation.guard.clone()?,
            actor: operation.actor.clone(),
            project: operation.project.clone(),
            outcome,
        })
    }
}

/// What an extension gives a network git operation
/// ([`Extension::git_env`](crate::Extension::git_env)): the environment
/// variables git runs with, and a short text naming what they provide, which
/// the failure message quotes when the remote refuses them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GitEnv {
    pub env: NodeEnv,
    /// `alice's token for git.example.com`; `None` names nothing more than the
    /// Actor.
    pub label: Option<String>,
}

impl GitEnv {
    /// These variables, labelled with nothing.
    pub fn new(env: NodeEnv) -> Self {
        Self { env, label: None }
    }

    /// The same variables, named by `label` in a failure message.
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Whether it gives no variable.
    pub fn is_empty(&self) -> bool {
        self.env.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operation_serializes_what_it_knows() {
        let op = GitOperation::new(
            Actor::LocalOperator,
            "https://git.example.com/acme/app.git",
            GitOperationKind::TriggerFetch,
        )
        .in_project("p1");
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({
                "actor": { "kind": "local_operator" }, "project": "p1",
                "url": "https://git.example.com/acme/app.git", "kind": "trigger_fetch",
            })
        );
    }

    #[test]
    fn a_guard_names_its_execution_and_its_end_repeats_it() {
        let op = GitOperation::guard(
            Actor::LocalOperator,
            "/repos/app",
            GuardRun::new("g1", Some("t1".into())),
        )
        .in_project("p1");
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({
                "actor": { "kind": "local_operator" }, "project": "p1",
                "url": "/repos/app", "kind": "guard",
                "guard": { "id": "g1", "trigger_id": "t1" },
            })
        );
        let end = GuardEnd::of(&op, GuardOutcome::TimedOut).unwrap();
        assert_eq!(
            serde_json::to_value(&end).unwrap(),
            serde_json::json!({
                "guard": { "id": "g1", "trigger_id": "t1" },
                "actor": { "kind": "local_operator" }, "project": "p1",
                "outcome": "timed_out",
            })
        );
        let clone = GitOperation::new(Actor::LocalOperator, "u", GitOperationKind::Clone);
        assert_eq!(GuardEnd::of(&clone, GuardOutcome::Passed), None);
    }

    #[test]
    fn an_env_carries_its_label() {
        let env = GitEnv::new(NodeEnv::from([("GIT_CONFIG_COUNT".into(), "1".into())]))
            .labelled("a token");
        assert!(!env.is_empty());
        assert_eq!(env.label.as_deref(), Some("a token"));
        assert!(GitEnv::default().is_empty());
    }
}
