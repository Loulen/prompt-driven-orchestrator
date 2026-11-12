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
    /// The remote's URL, as git is given it.
    pub url: String,
    pub kind: GitOperationKind,
}

impl GitOperation {
    /// `kind` on `url` for `actor`, in no Projet.
    pub fn new(actor: Actor, url: impl Into<String>, kind: GitOperationKind) -> Self {
        Self {
            actor,
            project: None,
            url: url.into(),
            kind,
        }
    }

    /// The same operation, in the Projet `project`.
    pub fn in_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
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
    fn an_env_carries_its_label() {
        let env = GitEnv::new(NodeEnv::from([("GIT_CONFIG_COUNT".into(), "1".into())]))
            .labelled("a token");
        assert!(!env.is_empty());
        assert_eq!(env.label.as_deref(), Some("a token"));
        assert!(GitEnv::default().is_empty());
    }
}
