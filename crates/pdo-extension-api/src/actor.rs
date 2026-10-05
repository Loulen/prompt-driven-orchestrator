use serde::{Deserialize, Serialize};

/// Who triggers a mutation (CONTEXT.md § *Extensions*).
///
/// Without an extension, every call comes from the [`Actor::LocalOperator`],
/// except the calls an agent's CLI makes from inside a node session
/// ([`Actor::Agent`]). An extension may identify the caller more precisely
/// ([`Actor::Identified`]).
///
/// Non-exhaustive: adding a kind of actor is a minor evolution of the API.
///
/// Deserializable since 0.8: the core stores the Actor that created a Run and
/// the last editor of a Trigger, and gives them back on the API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Actor {
    /// The person at the keyboard of this machine. The default actor.
    LocalOperator,
    /// A NodeRun, calling back through the agent CLI (`pdo complete`, `pdo fail`…).
    Agent { run_id: String, node_id: String },
    /// A Trigger firing on its schedule.
    Trigger { trigger_id: String },
    /// An actor established by an extension: `namespace` is the extension's id,
    /// `id` whatever that extension uses to name the caller.
    Identified { namespace: String, id: String },
}

impl Actor {
    /// A short, stable name of the kind of actor, for logs and messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Actor::LocalOperator => "local_operator",
            Actor::Agent { .. } => "agent",
            Actor::Trigger { .. } => "trigger",
            Actor::Identified { .. } => "identified",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_a_kind_tag() {
        let agent = Actor::Agent {
            run_id: "r1".into(),
            node_id: "n1".into(),
        };
        assert_eq!(
            serde_json::to_value(&agent).unwrap(),
            serde_json::json!({ "kind": "agent", "run_id": "r1", "node_id": "n1" })
        );
        assert_eq!(
            serde_json::to_value(Actor::LocalOperator).unwrap(),
            serde_json::json!({ "kind": "local_operator" })
        );
        assert_eq!(agent.kind(), "agent");
    }

    #[test]
    fn reads_back_what_it_writes() {
        for actor in [
            Actor::LocalOperator,
            Actor::Agent {
                run_id: "r1".into(),
                node_id: "n1".into(),
            },
            Actor::Trigger {
                trigger_id: "t1".into(),
            },
            Actor::Identified {
                namespace: "iam".into(),
                id: "alice".into(),
            },
        ] {
            let json = serde_json::to_value(&actor).unwrap();
            assert_eq!(serde_json::from_value::<Actor>(json).unwrap(), actor);
        }
    }
}
