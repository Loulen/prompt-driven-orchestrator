//! The internal test extension (ADR-0081, D17): the one guard that turns the core
//! red when it breaks its extension API. Internal to the test suite, never
//! published.
//!
//! - **`identify`**: the caller named by the [`ACTOR_HEADER`] request header
//!   becomes `Actor::Identified { namespace: "test-extension", id }`; without the
//!   header, the core decides.
//! - **`authorize`**: refuses what its [`Refusal`] says, with a reason naming the
//!   Operation. The refusal can change while the daemon runs ([`Control::set`]),
//!   so a test seeds its fixtures before refusing everything.
//! - Every `(Actor, Operation)` it is asked about is recorded ([`Control::seen`]).
//!
//! `demo_daemon.rs` serves it as a demo daemon for the Feature Paths.

use std::sync::{Arc, Mutex};

use pdo::extension_api::{
    Actor, Authorization, Extension, Manifest, Operation, RequestParts, ResourceKind, Verb,
};

/// The id of the test extension (its namespace for the actors it identifies).
pub const ID: &str = "test-extension";

/// The request header naming the caller.
pub const ACTOR_HEADER: &str = "x-pdo-test-actor";

/// What the test extension refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Nothing,
    Everything,
    /// One verb on one kind of resource.
    Only(ResourceKind, Verb),
    /// Panics in `authorize`, on every Operation.
    Panic,
}

impl Refusal {
    /// Read a refusal from its text form: `none`, `all`, `panic`, or
    /// `<resource>:<verb>` with the wire names of both (`project:update`,
    /// `trigger:fire` for the action `fire`).
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.trim() {
            "none" => Ok(Refusal::Nothing),
            "all" => Ok(Refusal::Everything),
            "panic" => Ok(Refusal::Panic),
            other => {
                let (resource, verb) = other.split_once(':').ok_or_else(|| {
                    format!("`{other}`: expected none, all, panic or resource:verb")
                })?;
                let resource = RESOURCES
                    .iter()
                    .copied()
                    .find(|r| resource_name(*r) == resource)
                    .ok_or_else(|| format!("unknown resource `{resource}`"))?;
                let verb = match verb {
                    "create" => Verb::Create,
                    "update" => Verb::Update,
                    "delete" => Verb::Delete,
                    action => Verb::Action(action.to_string()),
                };
                Ok(Refusal::Only(resource, verb))
            }
        }
    }

    fn refuses(&self, operation: &Operation) -> bool {
        match self {
            Refusal::Nothing => false,
            Refusal::Everything | Refusal::Panic => true,
            Refusal::Only(resource, verb) => {
                operation.resource == *resource && operation.verb == *verb
            }
        }
    }
}

/// The resources a text refusal can name.
const RESOURCES: &[ResourceKind] = &[
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

fn resource_name(resource: ResourceKind) -> String {
    serde_json::to_value(resource)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn verb_name(verb: &Verb) -> String {
    match verb {
        Verb::Create => "create".into(),
        Verb::Update => "update".into(),
        Verb::Delete => "delete".into(),
        Verb::Action(action) => action.clone(),
        other => format!("{other:?}"),
    }
}

/// The reason the test extension gives for refusing `operation`.
pub fn reason(operation: &Operation) -> String {
    format!(
        "the test extension refuses {} {}",
        verb_name(&operation.verb),
        resource_name(operation.resource)
    )
}

/// The test's side of the extension: change what it refuses, read what it saw.
#[derive(Clone, Default)]
pub struct Control {
    refusal: Arc<Mutex<Option<Refusal>>>,
    seen: Arc<Mutex<Vec<(Actor, Operation)>>>,
}

impl Control {
    pub fn set(&self, refusal: Refusal) {
        *self.refusal.lock().unwrap() = Some(refusal);
    }

    /// Every `(actor, operation)` `authorize` was asked about, in order.
    pub fn seen(&self) -> Vec<(Actor, Operation)> {
        self.seen.lock().unwrap().clone()
    }
}

pub struct TestExtension {
    control: Control,
    api_version: Option<String>,
}

impl TestExtension {
    /// The extension refusing `refusal`, and its control.
    pub fn new(refusal: Refusal) -> (Self, Control) {
        let control = Control::default();
        control.set(refusal);
        let ext = Self {
            control: control.clone(),
            api_version: None,
        };
        (ext, control)
    }

    /// The same extension, declaring that it requires `api_version`.
    pub fn requiring_api(mut self, api_version: &str) -> Self {
        self.api_version = Some(api_version.to_string());
        self
    }

    pub fn boxed(self) -> Vec<Box<dyn Extension>> {
        vec![Box::new(self)]
    }
}

impl Extension for TestExtension {
    fn manifest(&self) -> Manifest {
        let manifest = Manifest::new(ID, "Test extension", "0.0.0");
        match &self.api_version {
            Some(version) => manifest.requiring_api(version.clone()),
            None => manifest,
        }
    }

    fn identify(&self, request: &RequestParts) -> Option<Actor> {
        let caller = request.headers.get(ACTOR_HEADER)?.to_str().ok()?.trim();
        (!caller.is_empty()).then(|| Actor::Identified {
            namespace: ID.to_string(),
            id: caller.to_string(),
        })
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.control
            .seen
            .lock()
            .unwrap()
            .push((actor.clone(), operation.clone()));
        let refusal = self
            .control
            .refusal
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Refusal::Nothing);
        if refusal == Refusal::Panic {
            panic!("the test extension panics in authorize");
        }
        if refusal.refuses(operation) {
            Authorization::deny(reason(operation))
        } else {
            Authorization::Allow
        }
    }
}

#[test]
fn a_refusal_reads_from_its_text_form() {
    assert_eq!(
        Refusal::parse("project:update"),
        Ok(Refusal::Only(ResourceKind::Project, Verb::Update))
    );
    assert_eq!(
        Refusal::parse("trigger:fire"),
        Ok(Refusal::Only(
            ResourceKind::Trigger,
            Verb::Action("fire".into())
        ))
    );
    assert_eq!(Refusal::parse("all"), Ok(Refusal::Everything));
    assert!(Refusal::parse("nope:update").is_err());
    assert!(Refusal::parse("project").is_err());
    assert_eq!(
        reason(&Operation::new(ResourceKind::Project, Verb::Update)),
        "the test extension refuses update project"
    );
}
