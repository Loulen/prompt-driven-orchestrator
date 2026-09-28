use serde::Serialize;

/// The generic description of a mutation the core is about to execute: a
/// resource, a verb, and the target when it already exists (CONTEXT.md §
/// *Extensions*, D9).
///
/// Every mutation has one. An Operation is never named after a need
/// ("UpdateProfileAsAdmin"): the same three fields describe every case, so the
/// list of variants never describes the extensions the core expects.
///
/// Non-exhaustive, like its fields' enums: build one with [`Operation::new`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Operation {
    pub resource: ResourceKind,
    pub verb: Verb,
    /// The id of the entity acted on; `None` for a creation.
    pub target: Option<String>,
}

impl Operation {
    /// An Operation with no target (a creation, or an action on a collection).
    pub fn new(resource: ResourceKind, verb: Verb) -> Self {
        Self {
            resource,
            verb,
            target: None,
        }
    }

    /// The same Operation, aimed at the entity `id`.
    pub fn on(mut self, id: impl Into<String>) -> Self {
        self.target = Some(id.into());
        self
    }
}

/// The kind of entity an [`Operation`] acts on.
///
/// Non-exhaustive: adding a resource is a minor evolution of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResourceKind {
    Run,
    Node,
    Pipeline,
    LibraryEntry,
    Project,
    Trigger,
    AgentProfile,
    Skill,
    SkillFolder,
    SandboxProfile,
    InstanceSettings,
    PageMount,
    ReviewComment,
    Repository,
    Session,
    Update,
    /// The daemon's OS service unit (`pdo service install` / `uninstall`).
    Service,
    /// A Stats absorption: one row counting the data of others (Combine).
    StatsAbsorption,
    /// The price table the costs are priced with (its sync).
    PriceTable,
}

/// What an [`Operation`] does to its resource.
///
/// Non-exhaustive: the named [`Verb::Action`] carries everything that is neither
/// a creation, a modification nor a deletion (`"retry"`, `"fire"`, `"pause"`…).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verb {
    Create,
    Update,
    Delete,
    Action(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_targeted_operation() {
        let op = Operation::new(ResourceKind::AgentProfile, Verb::Update).on("p1");
        assert_eq!(op.target.as_deref(), Some("p1"));
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({ "resource": "agent_profile", "verb": "update", "target": "p1" })
        );
        let fire = Operation::new(ResourceKind::Trigger, Verb::Action("fire".into()));
        assert_eq!(
            serde_json::to_value(&fire).unwrap(),
            serde_json::json!({ "resource": "trigger", "verb": { "action": "fire" }, "target": null })
        );
    }
}
