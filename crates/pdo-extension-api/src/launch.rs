use std::collections::BTreeMap;

use serde::Serialize;

use crate::Actor;

/// The environment variables an extension adds to a Node
/// ([`Extension::node_env`](crate::Extension::node_env)): name → value.
pub type NodeEnv = BTreeMap<String, String>;

/// What a Node runs: an agent reads a preamble, a script reads none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NodeKind {
    /// An agent, launched on a harness (`claude`…), which reads the PDO
    /// preamble before its role prompt.
    Agent,
    /// A `script` node: bash, no preamble.
    Script,
}

/// The Trigger a Run was fired by, and who answered for it then.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub struct TriggerProvenance {
    /// The Trigger's id.
    pub id: String,
    /// Its last editor when it fired (CONTEXT.md § *Trigger*); `None` for a
    /// Trigger no one edited since the core started recording editors.
    pub editor: Option<Actor>,
}

impl TriggerProvenance {
    pub fn new(id: impl Into<String>, editor: Option<Actor>) -> Self {
        Self {
            id: id.into(),
            editor,
        }
    }
}

/// The launch of a Node, as the launch hooks see it
/// ([`Extension::node_env`](crate::Extension::node_env),
/// [`Extension::node_preamble`](crate::Extension::node_preamble)): which Run,
/// which Node, who created the Run, in which Projet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct NodeLaunch {
    pub run_id: String,
    pub node_id: String,
    /// The iteration launched, from 1.
    pub iter: i64,
    pub kind: NodeKind,
    /// The Actor of the Run's `run` × `create`: the caller of `POST /runs`, or
    /// the Trigger itself for a fired Run. `None` for a Run created before the
    /// core recorded it.
    pub created_by: Option<Actor>,
    /// The Trigger that fired the Run, if one did.
    pub trigger: Option<TriggerProvenance>,
    /// The Run's primary repository (its absolute path).
    pub repository: String,
    /// The id of the Projet owning that repository, if one does.
    pub project: Option<String>,
}

impl NodeLaunch {
    /// The launch of iteration `iter` of `node_id`, in Run `run_id` on
    /// `repository`; nobody known as its creator, no Trigger, no Projet.
    pub fn new(
        run_id: impl Into<String>,
        node_id: impl Into<String>,
        iter: i64,
        kind: NodeKind,
        repository: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            node_id: node_id.into(),
            iter,
            kind,
            created_by: None,
            trigger: None,
            repository: repository.into(),
            project: None,
        }
    }

    /// The same launch, of a Run `actor` created.
    pub fn created_by(mut self, actor: Actor) -> Self {
        self.created_by = Some(actor);
        self
    }

    /// The same launch, of a Run `trigger` fired.
    pub fn fired_by(mut self, trigger: TriggerProvenance) -> Self {
        self.trigger = Some(trigger);
        self
    }

    /// The same launch, in the Projet `project`.
    pub fn in_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }
}

/// A Trigger, as [`Extension::trigger_suspension`](crate::Extension::trigger_suspension)
/// sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct TriggerRef {
    pub id: String,
    pub name: String,
    /// The Actor of its last creation or edit; `None` for a Trigger no one
    /// edited since the core started recording editors.
    pub last_editor: Option<Actor>,
}

impl TriggerRef {
    pub fn new(id: impl Into<String>, name: impl Into<String>, last_editor: Option<Actor>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            last_editor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_serializes_what_it_knows() {
        let launch = NodeLaunch::new("r1", "n1", 2, NodeKind::Agent, "/repo")
            .created_by(Actor::Trigger {
                trigger_id: "t1".into(),
            })
            .fired_by(TriggerProvenance::new("t1", Some(Actor::LocalOperator)))
            .in_project("p1");
        assert_eq!(
            serde_json::to_value(&launch).unwrap(),
            serde_json::json!({
                "run_id": "r1", "node_id": "n1", "iter": 2, "kind": "agent",
                "created_by": { "kind": "trigger", "trigger_id": "t1" },
                "trigger": { "id": "t1", "editor": { "kind": "local_operator" } },
                "repository": "/repo", "project": "p1",
            })
        );
    }
}
