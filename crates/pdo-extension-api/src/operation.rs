use std::fmt;

use serde::{Serialize, Serializer};

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

/// The kind of entity an [`Operation`] acts on: one of the core's, or one an
/// extension owns ([`ResourceKind::Extension`]).
///
/// On the wire, its name: `run`, `agent_profile`… for the core's kinds,
/// `<extension>/<kind>` for an extension's (`pm/tache`).
///
/// Non-exhaustive: adding a resource is a minor evolution of the API.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// A kind of entity an extension owns and mutates itself (a Tâche of the
    /// extension `pm`: `extension: "pm"`, `kind: "tache"`). Its Operations go
    /// through the same Autorisation d'opération as the core's, by the
    /// extension's [`Host`](crate::Host).
    Extension {
        /// The id of the extension that owns the entities.
        extension: String,
        /// The kind of entity, as that extension names it.
        kind: String,
    },
}

impl ResourceKind {
    /// The kind `kind` of entity the extension `extension` owns.
    pub fn extension(extension: impl Into<String>, kind: impl Into<String>) -> Self {
        ResourceKind::Extension {
            extension: extension.into(),
            kind: kind.into(),
        }
    }

    /// The name on the wire: `agent_profile`, or `<extension>/<kind>`.
    pub fn wire_name(&self) -> String {
        let core = match self {
            ResourceKind::Run => "run",
            ResourceKind::Node => "node",
            ResourceKind::Pipeline => "pipeline",
            ResourceKind::LibraryEntry => "library_entry",
            ResourceKind::Project => "project",
            ResourceKind::Trigger => "trigger",
            ResourceKind::AgentProfile => "agent_profile",
            ResourceKind::Skill => "skill",
            ResourceKind::SkillFolder => "skill_folder",
            ResourceKind::SandboxProfile => "sandbox_profile",
            ResourceKind::InstanceSettings => "instance_settings",
            ResourceKind::PageMount => "page_mount",
            ResourceKind::ReviewComment => "review_comment",
            ResourceKind::Repository => "repository",
            ResourceKind::Session => "session",
            ResourceKind::Update => "update",
            ResourceKind::Service => "service",
            ResourceKind::StatsAbsorption => "stats_absorption",
            ResourceKind::PriceTable => "price_table",
            ResourceKind::Extension { extension, kind } => return format!("{extension}/{kind}"),
        };
        core.to_string()
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.wire_name())
    }
}

impl Serialize for ResourceKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.wire_name())
    }
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

    #[test]
    fn an_extension_resource_is_named_after_its_extension() {
        let op = Operation::new(ResourceKind::extension("pm", "tache"), Verb::Update).on("PM-12");
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({ "resource": "pm/tache", "verb": "update", "target": "PM-12" })
        );
        assert_eq!(
            ResourceKind::extension("pm", "tache"),
            ResourceKind::Extension {
                extension: "pm".into(),
                kind: "tache".into()
            }
        );
        assert_eq!(ResourceKind::extension("pm", "page").to_string(), "pm/page");
        assert_eq!(
            ResourceKind::StatsAbsorption.wire_name(),
            "stats_absorption"
        );
    }
}
