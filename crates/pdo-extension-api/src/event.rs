use serde::Serialize;

use crate::{Actor, EntityRef, Operation};

/// The `type` of the event the core hands to [`Extension::on_event`] after each
/// authorized and successful mutation.
///
/// [`Extension::on_event`]: crate::Extension::on_event
pub const OPERATION_APPLIED: &str = "operation_applied";

/// An authorized mutation that succeeded: who ran which Operation, on which
/// entity. Broadcast on `/ws` too once an extension is loaded (amendment #34):
/// without extension, `/ws` carries exactly what it carried before the API.
///
/// The core builds it for its own mutations; an extension builds it for its
/// own and hands it to [`Host::applied`](crate::Host::applied).
///
/// On the wire: `{"type": "operation_applied", "actor": …, "operation": …,
/// "entity": {"kind": …, "id": …} | null}`. The entity is the target of the
/// Operation, or the entity a creation made when the core knows it; `null`
/// otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct OperationApplied {
    pub actor: Actor,
    pub operation: Operation,
    pub entity: Option<EntityRef>,
}

impl OperationApplied {
    pub fn new(actor: Actor, operation: Operation, entity: Option<EntityRef>) -> Self {
        Self {
            actor,
            operation,
            entity,
        }
    }

    /// The event as `on_event` receives it.
    pub fn to_event(&self) -> serde_json::Value {
        let mut event = serde_json::json!({ "type": OPERATION_APPLIED });
        if let (Some(event), Ok(serde_json::Value::Object(fields))) =
            (event.as_object_mut(), serde_json::to_value(self))
        {
            event.extend(fields);
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceKind, Verb};

    #[test]
    fn carries_its_type_the_actor_the_operation_and_the_entity() {
        let applied = OperationApplied::new(
            Actor::LocalOperator,
            Operation::new(ResourceKind::Project, Verb::Update)
                .on("p1")
                .in_projects(["p1"]),
            Some(EntityRef::new(ResourceKind::Project, "p1")),
        );
        assert_eq!(
            applied.to_event(),
            serde_json::json!({
                "type": "operation_applied",
                "actor": { "kind": "local_operator" },
                "operation": {
                    "resource": "project", "verb": "update", "target": "p1", "projects": ["p1"],
                },
                "entity": { "kind": "project", "id": "p1" },
            })
        );
    }
}
