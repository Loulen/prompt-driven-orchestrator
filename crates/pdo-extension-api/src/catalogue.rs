use serde::Serialize;

use crate::{Manifest, ResourceKind, Verb};

/// A Ressource an extension declares in its [`Manifest`]: a kind of entity it
/// owns and mutates through its [`Host`](crate::Host), so that an extension of
/// rights can set rights on it without knowing the extension.
///
/// `kind` is the kind as the extension names it in its
/// [`ResourceKind::Extension`] (`note`); in the catalogue, the Ressource is
/// `<extension id>/<kind>` (`test/note`). `verbs` are the names of the verbs its
/// Operations use: `create`, `update`, `delete`, or an action's name
/// ([`Verb::name`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ResourceDeclaration {
    pub kind: String,
    /// What the interface calls it, plural (`Notes`).
    pub label: String,
    /// One line of help, empty by default.
    pub description: String,
    pub verbs: Vec<String>,
}

impl ResourceDeclaration {
    pub fn new<I, S>(kind: impl Into<String>, label: impl Into<String>, verbs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            kind: kind.into(),
            label: label.into(),
            description: String::new(),
            verbs: verbs.into_iter().map(Into::into).collect(),
        }
    }

    /// The same declaration, with one line of help.
    pub fn described(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

/// One Ressource of the catalogue (CONTEXT.md § *Extensions*): one of the
/// core's, or one an extension declares. What an extension of rights lists to
/// set rights on, without knowing the others.
///
/// On the wire: `{"resource": "test/note", "label": "Notes", "description":
/// "…", "verbs": ["create", …], "extension": "test"}`; `extension` is `null` for
/// the core's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Resource {
    /// The Ressource's name, as an [`Operation`](crate::Operation) carries
    /// it: [`ResourceKind::wire_name`].
    pub resource: String,
    pub label: String,
    pub description: String,
    pub verbs: Vec<String>,
    /// The id of the extension that declares it; `None` for the core's.
    pub extension: Option<String>,
}

impl Resource {
    /// The core's Ressource `kind`; `None` for an extension's kind.
    pub fn of_core(kind: &ResourceKind) -> Option<Self> {
        let (label, description, verbs) = core_entry(kind)?;
        Some(Self {
            resource: kind.wire_name(),
            label: label.to_string(),
            description: description.to_string(),
            verbs: verbs.iter().map(|verb| verb.to_string()).collect(),
            extension: None,
        })
    }

    /// The Ressource the extension `extension` declares with `declaration`.
    pub fn of_extension(extension: &str, declaration: &ResourceDeclaration) -> Self {
        Self {
            resource: ResourceKind::extension(extension, declaration.kind.clone()).wire_name(),
            label: declaration.label.clone(),
            description: declaration.description.clone(),
            verbs: declaration.verbs.clone(),
            extension: Some(extension.to_string()),
        }
    }
}

/// The core's Ressources, in the order of [`ResourceKind::CORE`].
pub fn core_resources() -> Vec<Resource> {
    ResourceKind::CORE
        .iter()
        .filter_map(Resource::of_core)
        .collect()
}

/// The whole catalogue: the core's Ressources, then each extension's, in the
/// order of `manifests` (registration order) and of their declarations.
pub fn catalogue<'a>(manifests: impl IntoIterator<Item = &'a Manifest>) -> Vec<Resource> {
    let mut resources = core_resources();
    for manifest in manifests {
        resources.extend(
            manifest
                .resources
                .iter()
                .map(|declaration| Resource::of_extension(&manifest.id, declaration)),
        );
    }
    resources
}

const CREATE: &str = "create";
const UPDATE: &str = "update";
const DELETE: &str = "delete";
const READ: &str = "read";

/// Label, help and verbs of each of the core's Ressources: every verb a use
/// case of the core builds an Operation with.
fn core_entry(
    kind: &ResourceKind,
) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    Some(match kind {
        ResourceKind::Run => (
            "Runs",
            "See, create, steer, archive and delete Runs",
            &[
                READ,
                CREATE,
                DELETE,
                "edit_repos",
                "edit_pipeline",
                "start_manager",
                "stop_manager",
                "bump_region",
                "end_region",
                "set_region_max_iter",
                "force_route",
                "pause_run",
                "resume_run",
                "cancel_run",
                "reopen_run",
                "inject_artifact",
                "rename_run",
                "cleanup_run",
                "retry_all",
            ],
        ),
        ResourceKind::Node => (
            "Nodes",
            "Start, stop, retry, skip and complete the nodes of a Run; import files and send text into their terminal",
            &[
                "start",
                "stop",
                "skip",
                "retry",
                "fail",
                "complete",
                "release_completion",
                "wait_user",
                "resume",
                "import_files",
                "send_text",
                "mark_node_done",
                "release_node_completion",
                "extend_cycle",
                "skip_node",
                "kill_node",
                "restart_node",
                "recover_node",
                "start_node",
            ],
        ),
        ResourceKind::Pipeline => (
            "Pipelines",
            "Create, edit, validate and delete the pipeline templates; open the library assistant",
            &[CREATE, UPDATE, DELETE, "validate", "open_assistant"],
        ),
        ResourceKind::LibraryEntry => (
            "Library entries",
            "Add and remove the pipelines of the library",
            &[CREATE, DELETE],
        ),
        ResourceKind::Project => (
            "Projects",
            "See, create, edit and delete Projects and their repositories",
            &[READ, CREATE, UPDATE, DELETE],
        ),
        ResourceKind::Trigger => (
            "Triggers",
            "Create, edit, fire and pause Triggers",
            &[CREATE, UPDATE, DELETE, "fire", "pause", "test_guard"],
        ),
        ResourceKind::AgentProfile => (
            "Agent profiles",
            "Create, edit and delete the agentic profiles",
            &[CREATE, UPDATE, DELETE],
        ),
        ResourceKind::Skill => (
            "Skills",
            "Create, edit, import and delete the skills of the bank",
            &[
                CREATE,
                UPDATE,
                DELETE,
                "scan",
                "cancel_scan",
                "import",
                "update_from_source",
            ],
        ),
        ResourceKind::SkillFolder => (
            "Skill folders",
            "Create, edit, rescan and delete the folders of the bank",
            &[CREATE, UPDATE, DELETE, "rescan"],
        ),
        ResourceKind::SandboxProfile => (
            "Sandbox profiles",
            "Create, edit and delete the sandbox profiles",
            &[CREATE, UPDATE, DELETE],
        ),
        ResourceKind::InstanceSettings => (
            "Instance settings",
            "Change the settings of the instance; probe the models of a harness again",
            &[UPDATE, "refresh_catalogue"],
        ),
        ResourceKind::PageMount => (
            "Mounted pages",
            "Mount and unmount static pages",
            &[CREATE, DELETE],
        ),
        ResourceKind::ReviewComment => (
            "Review comments",
            "Send, answer, resolve and reopen the review comments of a Run",
            &[CREATE, "reply", "resolve", "reopen"],
        ),
        ResourceKind::Repository => (
            "Repositories",
            "Browse the disk for a repository, create repositories, fetch their remotes, fast-forward their branches",
            &[READ, CREATE, "fetch", "fast_forward"],
        ),
        ResourceKind::Session => (
            "Sessions",
            "Open, attach and take control of terminals; open, read, relaunch, configure, zoom, type into, terminate and forget the hosted sessions",
            &[
                READ,
                CREATE,
                UPDATE,
                DELETE,
                "attach",
                "take_control",
                "resume",
                "open",
                "terminate",
                "relaunch",
                "configure",
                "zoom",
                "type",
                "forget",
                "wait_user",
            ],
        ),
        ResourceKind::Update => (
            "Updates",
            "Check for and apply updates",
            &["check", "apply"],
        ),
        ResourceKind::Service => (
            "Service",
            "Install and uninstall the daemon as an OS service",
            &[CREATE, DELETE],
        ),
        ResourceKind::StatsAbsorption => (
            "Stats combinations",
            "Combine and split rows of the Stats",
            &[CREATE, DELETE],
        ),
        ResourceKind::PriceTable => (
            "Price table",
            "Sync the price table the costs are priced with",
            &["sync"],
        ),
        ResourceKind::Extension { .. } => return None,
    })
}

impl Verb {
    /// The verb's name, as the catalogue lists it: `read`, `create`,
    /// `update`, `delete`, or the action's own name (`fire`).
    pub fn name(&self) -> &str {
        match self {
            Verb::Read => READ,
            Verb::Create => CREATE,
            Verb::Update => UPDATE,
            Verb::Delete => DELETE,
            Verb::Action(action) => action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_lists_each_of_its_resources_with_a_label_and_its_verbs() {
        let core = core_resources();
        assert_eq!(core.len(), ResourceKind::CORE.len());
        assert_eq!(core.len(), 19);
        for resource in &core {
            assert!(!resource.label.is_empty(), "{resource:?}");
            assert!(!resource.description.is_empty(), "{resource:?}");
            assert!(!resource.verbs.is_empty(), "{resource:?}");
            assert_eq!(resource.extension, None);
        }
        let run = &core[0];
        assert_eq!(run.resource, "run");
        assert!(run.verbs.contains(&"create".to_string()));
        assert_eq!(run.verbs[0], "read", "a Run is read (CORE-5)");
        let readable: Vec<&str> = core
            .iter()
            .filter(|r| r.verbs.iter().any(|v| v == "read"))
            .map(|r| r.resource.as_str())
            .collect();
        // A session is read too (CORE-124): its events on `/ws`, its Zoom du terminal.
        assert_eq!(readable, ["run", "project", "repository", "session"]);
        assert!(Resource::of_core(&ResourceKind::extension("test", "note")).is_none());
    }

    #[test]
    fn the_catalogue_appends_each_extensions_declarations_in_order() {
        let test = Manifest::new("test", "Test", "1.0.0")
            .with_resource(
                ResourceDeclaration::new("folder", "Folders", ["create", "update"])
                    .described("The folders"),
            )
            .with_resource(ResourceDeclaration::new(
                "note",
                "Notes",
                ["create", "archive"],
            ));
        let bare = Manifest::new("bare", "Bare", "0.0.0");
        let catalogue = catalogue([&test, &bare]);
        assert_eq!(catalogue.len(), 21);
        let note = catalogue.last().unwrap();
        assert_eq!(
            serde_json::to_value(note).unwrap(),
            serde_json::json!({
                "resource": "test/note",
                "label": "Notes",
                "description": "",
                "verbs": ["create", "archive"],
                "extension": "test",
            })
        );
        assert_eq!(catalogue[19].resource, "test/folder");
        assert_eq!(catalogue[19].description, "The folders");
        assert_eq!(
            serde_json::to_value(&catalogue[0]).unwrap()["extension"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn a_verb_is_named_as_the_catalogue_lists_it() {
        assert_eq!(Verb::Read.name(), "read");
        assert_eq!(Verb::Create.name(), "create");
        assert_eq!(Verb::Update.name(), "update");
        assert_eq!(Verb::Delete.name(), "delete");
        assert_eq!(Verb::Action("fire".into()).name(), "fire");
    }
}
