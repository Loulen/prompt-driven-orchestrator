use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The generic description of a mutation the core is about to execute, or of
/// a read it is about to serve ([`Verb::Read`]): a resource, a verb, and the
/// target when it already exists (CONTEXT.md § *Extensions*, D9).
///
/// Every mutation has one, and so do the guarded reads (CORE-5): listing and
/// reading the Runs, listing and reading the Projets, browsing the disk. An Operation is never named after a need
/// ("UpdateProfileAsAdmin"): the same fields describe every case, so the list
/// of variants never describes the extensions the core expects.
///
/// It also says which Projets it is in ([`Operation::projects`]): the core
/// fills them in its use cases, an extension for its own resources, so that no
/// extension ever works out a Projet from a repository itself.
///
/// Non-exhaustive, like its fields' enums: build one with [`Operation::new`].
///
/// It reads back from its wire form (`{"resource", "verb", "target",
/// "projects"}`, the last two optional): the form the question of
/// authorization (`POST /authorize`) receives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Operation {
    pub resource: ResourceKind,
    pub verb: Verb,
    /// The id of the entity acted on; `None` for a creation.
    #[serde(default)]
    pub target: Option<String>,
    /// The ids of the Projets the Operation is in, without duplicates; empty
    /// when it is in none (hors Projet).
    ///
    /// The core fills them for its own Operations: a Run's, its nodes', its
    /// sessions' and its review comments' are the Projet recorded on the Run
    /// at its launch (ADR-0087; a creation: the Projet it names, else its
    /// parent Run's); a Trigger's, the Projet it launches its Runs in; a
    /// Projet's are that Projet; a repository's, every Projet it is a member
    /// of. The others (pipelines, settings, skills…) are in none. An extension
    /// fills them for its own resources (a note may belong to several
    /// Projets).
    #[serde(default)]
    pub projects: Vec<String>,
}

impl Operation {
    /// An Operation with no target (a creation, or an action on a collection).
    pub fn new(resource: ResourceKind, verb: Verb) -> Self {
        Self {
            resource,
            verb,
            target: None,
            projects: Vec::new(),
        }
    }

    /// The same Operation, aimed at the entity `id`.
    pub fn on(mut self, id: impl Into<String>) -> Self {
        self.target = Some(id.into());
        self
    }

    /// The same Operation, in the Projets `projects` (in place of the ones it
    /// had), a duplicate kept once. An empty list: in no Projet.
    pub fn in_projects<I, S>(mut self, projects: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.projects = Vec::new();
        for project in projects {
            let project = project.into();
            if !self.projects.contains(&project) {
                self.projects.push(project);
            }
        }
        self
    }

    /// Whether the Operation is in no Projet.
    pub fn is_outside_projects(&self) -> bool {
        self.projects.is_empty()
    }

    /// Whether it is a read ([`Verb::Read`]): it changes nothing, so it is
    /// never announced as applied.
    pub fn is_read(&self) -> bool {
        self.verb == Verb::Read
    }
}

/// The kind of entity an [`Operation`] acts on: one of the core's, or one an
/// extension owns ([`ResourceKind::Extension`]).
///
/// On the wire, its name: `run`, `agent_profile`… for the core's kinds,
/// `<extension>/<kind>` for an extension's (`test/note`).
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
    /// A kind of entity an extension owns and mutates itself (a note of the
    /// extension `test`: `extension: "test"`, `kind: "note"`). Its Operations go
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
    /// Every kind of the core, in the catalogue's order.
    pub const CORE: &'static [ResourceKind] = &[
        ResourceKind::Run,
        ResourceKind::Node,
        ResourceKind::Pipeline,
        ResourceKind::LibraryEntry,
        ResourceKind::Project,
        ResourceKind::Trigger,
        ResourceKind::AgentProfile,
        ResourceKind::Skill,
        ResourceKind::SkillFolder,
        ResourceKind::SandboxProfile,
        ResourceKind::InstanceSettings,
        ResourceKind::PageMount,
        ResourceKind::ReviewComment,
        ResourceKind::Repository,
        ResourceKind::Session,
        ResourceKind::Update,
        ResourceKind::Service,
        ResourceKind::StatsAbsorption,
        ResourceKind::PriceTable,
    ];

    /// The kind whose wire name is `name`: one of [`ResourceKind::CORE`], or
    /// `<extension>/<kind>` for an extension's. `None` for an unknown core
    /// name or an empty part.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name.split_once('/') {
            Some((extension, kind)) if !extension.is_empty() && !kind.is_empty() => {
                Some(ResourceKind::extension(extension, kind))
            }
            Some(_) => None,
            None => ResourceKind::CORE
                .iter()
                .find(|kind| kind.wire_name() == name)
                .cloned(),
        }
    }

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

impl<'de> Deserialize<'de> for ResourceKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        ResourceKind::from_wire_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown resource `{name}`")))
    }
}

/// What an [`Operation`] does to its resource.
///
/// Non-exhaustive: the named [`Verb::Action`] carries everything that is neither
/// a creation, a modification nor a deletion (`"retry"`, `"fire"`, `"pause"`…).
///
/// [`Verb::Read`] (CORE-5) is the one verb that changes nothing: a Run listed
/// or read, a Projet listed or read, the disk browsed. A refused read hides
/// the entity from a list, and answers `403` for one entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verb {
    Create,
    Update,
    Delete,
    Action(String),
    Read,
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
            serde_json::json!({
                "resource": "agent_profile", "verb": "update", "target": "p1", "projects": [],
            })
        );
        let fire = Operation::new(ResourceKind::Trigger, Verb::Action("fire".into()));
        assert_eq!(
            serde_json::to_value(&fire).unwrap(),
            serde_json::json!({
                "resource": "trigger", "verb": { "action": "fire" }, "target": null, "projects": [],
            })
        );
    }

    #[test]
    fn an_operation_is_outside_every_projet_until_given_some() {
        let op = Operation::new(ResourceKind::Run, Verb::Delete).on("r1");
        assert!(op.is_outside_projects());
        let op = op.in_projects(["p1", "p2", "p1"]);
        assert_eq!(op.projects, vec!["p1".to_string(), "p2".to_string()]);
        assert!(!op.is_outside_projects());
        assert_eq!(
            serde_json::to_value(&op).unwrap()["projects"],
            serde_json::json!(["p1", "p2"])
        );
        assert!(op.in_projects(Vec::<String>::new()).is_outside_projects());
    }

    #[test]
    fn an_extension_resource_is_named_after_its_extension() {
        let op =
            Operation::new(ResourceKind::extension("test", "note"), Verb::Update).on("note-12");
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({
                "resource": "test/note", "verb": "update", "target": "note-12", "projects": [],
            })
        );
        assert_eq!(
            ResourceKind::extension("test", "note"),
            ResourceKind::Extension {
                extension: "test".into(),
                kind: "note".into()
            }
        );
        assert_eq!(
            ResourceKind::extension("test", "page").to_string(),
            "test/page"
        );
        assert_eq!(
            ResourceKind::StatsAbsorption.wire_name(),
            "stats_absorption"
        );
    }

    #[test]
    fn an_operation_reads_back_from_its_wire_form() {
        let op: Operation = serde_json::from_value(serde_json::json!({
            "resource": "run", "verb": "create", "projects": ["p1"],
        }))
        .unwrap();
        assert_eq!(
            op,
            Operation::new(ResourceKind::Run, Verb::Create).in_projects(["p1"])
        );
        let read = Operation::new(ResourceKind::Run, Verb::Read).on("r1");
        assert!(read.is_read());
        assert!(!op.is_read());
        assert_eq!(serde_json::to_value(&read).unwrap()["verb"], "read");
        assert_eq!(
            serde_json::from_value::<Operation>(serde_json::to_value(&read).unwrap()).unwrap(),
            read
        );
        let fire: Operation = serde_json::from_value(serde_json::json!({
            "resource": "trigger", "verb": { "action": "fire" }, "target": "t1",
        }))
        .unwrap();
        assert_eq!(fire.verb, Verb::Action("fire".into()));
        let note: Operation =
            serde_json::from_value(serde_json::json!({ "resource": "test/note", "verb": "read" }))
                .unwrap();
        assert_eq!(note.resource, ResourceKind::extension("test", "note"));
        assert!(serde_json::from_value::<Operation>(
            serde_json::json!({ "resource": "nope", "verb": "read" })
        )
        .is_err());
    }

    #[test]
    fn a_kind_reads_back_from_its_wire_name() {
        for kind in ResourceKind::CORE {
            assert_eq!(
                ResourceKind::from_wire_name(&kind.wire_name()).as_ref(),
                Some(kind)
            );
        }
        assert_eq!(
            ResourceKind::from_wire_name("test/note"),
            Some(ResourceKind::extension("test", "note"))
        );
        assert_eq!(ResourceKind::from_wire_name("nope"), None);
        assert_eq!(ResourceKind::from_wire_name("test/"), None);
    }
}
