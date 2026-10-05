//! The internal test extension (ADR-0081, D17): the one guard that turns the core
//! red when it breaks its extension API. Internal to the test suite, never
//! published.
//!
//! - **`identify`**: the caller named by the [`ACTOR_HEADER`] request header
//!   becomes `Actor::Identified { namespace: "test", id }`; without the
//!   header, the core decides.
//! - **`authorize`**: refuses what its [`Refusal`] says, with a reason naming the
//!   Operation. The refusal can change while the daemon runs ([`Control::set`]),
//!   so a test seeds its fixtures before refusing everything.
//! - Every `(Actor, Operation)` it is asked about is recorded ([`Control::seen`]).
//! - **`on_create`**: stamps `started_by` (so `test/started_by`): the id of an
//!   identified caller, else the kind of actor (`local_operator`, `trigger`…).
//! - **`on_event`**: records every event ([`Control::events`]) and appends it
//!   to `events.jsonl` in its data folder; it can be told to sleep or panic
//!   first ([`OnEvent`]).
//! - **`start`**: remembers its data folder ([`Control::data_dir`]).
//! - **`routes`** (`routes.rs`): `/ext/test` answers who it is, `/events` the
//!   trace of the events, `/ui.js` its UI bundle, `/icon.svg` its icon,
//!   `/commands` the trace of its command's calls (`POST` records one),
//!   `/incompatible` switches the bundle to a React major the front does not run.
//! - **`ui`** (`ui.js`): one element at each location of the catalogue. The
//!   four locations of 0.1 through their 0.1 slots (`Navigation`,
//!   `SettingsPanel`, `ListFilter`/`DetailTab` of the Runs), so they prove the
//!   0.1 form still renders; `topbar/end` through 0.2 contributions: a page with
//!   its icon, an action calling [`SHOW_CONTEXT`] while a Run is selected, and
//!   an action whose `when` never holds. Every Action location (#30) through
//!   one action each ([`ACTION_LOCATIONS`]), titled `Test <location>`, calling
//!   the same function; the one of `run/item/menu` only while the row's Run is
//!   not archived, and that menu also offers [`FAIL`], a command that throws.
//!   Since #31, one element at each other location: a tab (`EntityTab`), a
//!   section (`EntitySection`, which records its host's Save in `/sections`), a
//!   decoration (`EntityBadge`; plus one that throws, `Exploding`, on the status
//!   bar) and a list filter (`StartedByFilter`) wherever the catalogue accepts
//!   one.
//! - **`cli`** / **`run_cli`**: `pdo ext test echo|cat|exit|panic` (see
//!   [`TestExtension::run_cli`]); each call is recorded ([`Control::cli_calls`]).
//! - **`skills`**: one skill, `test-extension-cli` (`skill/`), at the
//!   extension's version.
//! - **`node_env`** / **`node_preamble`** / **`trigger_suspension`** (API
//!   0.8, `launch.rs`): the variables, the preamble and the suspensions the
//!   test sets ([`Control::launch`]).
//! - [`SecondTestExtension`] (`test2`): a second extension with a section at
//!   `project/edit`, so two extensions share a location.
//!
//! Its peers of `interop.rs` run their own mutations through the [`Host`] the
//! core hands them (amendment #34): the test extension sees their Operations
//! in `authorize`, their events in `on_event`, stamps their entities.
//!
//! `hosted.rs` adds two more, **A** and **B**, for the locations an extension
//! hosts (amendment #32): A hosts `a/panneau` and `a/actions`, B contributes.
//! Their ids are those of the interop peers: a daemon loads one set or the other.
//!
//! `demo_daemon.rs` serves it as a demo daemon for the Feature Paths.
//!
//! [`Host`]: pdo::extension_api::Host

pub mod hosted;
pub mod interop;
pub mod launch;
mod routes;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdo::extension_api::{
    Actor, Attributes, Authorization, CliInvocation, CliOutput, CliSpec, EntityRef, Extension,
    ExtensionContext, ExtensionSkill, Manifest, NodeEnv, NodeLaunch, Operation, RequestParts,
    ResourceKind, Slot, TriggerRef, UiCommand, UiContribution, UiContributions, UiLocation, UiSlot,
    Verb, When,
};
use serde_json::Value;

/// The id of the test extension (its namespace for the actors it identifies,
/// the prefix of its attributes, its routes under `/ext/test/`).
pub const ID: &str = "test";

/// The major of React the test bundle declares; the incompatible one declares
/// the one before.
pub const REACT_MAJOR: u32 = 19;

/// The request header naming the caller.
pub const ACTOR_HEADER: &str = "x-pdo-test-actor";

/// The id of the second test extension.
pub const SECOND_ID: &str = "test2";

/// The version of the test extension, which its skill is seeded at.
pub const VERSION: &str = "0.0.0";

/// The name of the skill the test extension provides.
pub const SKILL_NAME: &str = "test-extension-cli";
/// Its `SKILL.md`, and its one reference file.
pub const SKILL_MD: &str = include_str!("skill/SKILL.md");
pub const SKILL_REFERENCE: (&str, &str) = (
    "references/commands.md",
    include_str!("skill/references/commands.md"),
);

/// How the test extension names an actor on the stderr of its command line.
pub fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::Agent { run_id, node_id } => format!("agent:{run_id}/{node_id}"),
        Actor::Identified { namespace, id } => format!("identified:{namespace}/{id}"),
        other => other.kind().to_string(),
    }
}

/// The id of the test extension's command: it reports the context it was
/// called with (a toast, and `POST /ext/test/commands`).
pub const SHOW_CONTEXT: &str = "test.show-context";

/// The id of the test extension's command that always throws: its error
/// reaches a toast, and the menu it sits in keeps working.
pub const FAIL: &str = "test.fail";

/// The Action locations (#30) the test extension contributes one action to,
/// in declaration order.
pub const ACTION_LOCATIONS: &[UiLocation] = &[
    UiLocation::RunsHeader,
    UiLocation::RunItemMenu,
    UiLocation::RunsBulk,
    UiLocation::RunToolbar,
    UiLocation::CanvasNodeMenu,
    UiLocation::CanvasEdgeMenu,
    UiLocation::NodeToolbar,
    UiLocation::TriggerItemMenu,
    UiLocation::PipelineItemMenu,
    UiLocation::PipelinesHeader,
    UiLocation::TabMenu,
    UiLocation::ReviewToolbar,
    UiLocation::ReviewFile,
    UiLocation::SkillMenu,
];

/// The id of the action the test extension contributes at `location`
/// (`at-run-item-menu` for `run/item/menu`).
pub fn action_id(location: UiLocation) -> String {
    format!("at-{}", location.id().replace('/', "-"))
}

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
    /// `trigger:fire` for the action `fire`, `a/item:create` for an
    /// extension's resource).
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.trim() {
            "none" => Ok(Refusal::Nothing),
            "all" => Ok(Refusal::Everything),
            "panic" => Ok(Refusal::Panic),
            other => {
                let (resource, verb) = other.split_once(':').ok_or_else(|| {
                    format!("`{other}`: expected none, all, panic or resource:verb")
                })?;
                let resource = match resource.split_once('/') {
                    Some((extension, kind)) if !extension.is_empty() && !kind.is_empty() => {
                        ResourceKind::extension(extension, kind)
                    }
                    _ => RESOURCES
                        .iter()
                        .find(|r| r.wire_name() == resource)
                        .cloned()
                        .ok_or_else(|| format!("unknown resource `{resource}`"))?,
                };
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
        operation.resource
    )
}

/// What the test extension does in `on_event` before it records the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnEvent {
    Record,
    /// Sleep that long first.
    Sleep(Duration),
    /// Panic instead of recording.
    Panic,
}

/// The test's side of the extension: change what it refuses, read what it saw.
#[derive(Clone)]
pub struct Control {
    refusal: Arc<Mutex<Option<Refusal>>>,
    seen: Arc<Mutex<Vec<(Actor, Operation)>>>,
    events: Arc<Mutex<Vec<Value>>>,
    on_event: Arc<Mutex<OnEvent>>,
    data_dir: Arc<Mutex<Option<PathBuf>>>,
    incompatible: Arc<AtomicBool>,
    cli_calls: Arc<Mutex<Vec<CliInvocation>>>,
    commands: Arc<Mutex<Vec<Value>>>,
    sections: Arc<Mutex<Vec<Value>>>,
    /// Its launch hooks and its suspensions of Triggers (API 0.8).
    pub launch: launch::Launch,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            refusal: Arc::default(),
            seen: Arc::default(),
            events: Arc::default(),
            on_event: Arc::new(Mutex::new(OnEvent::Record)),
            data_dir: Arc::default(),
            incompatible: Arc::default(),
            cli_calls: Arc::default(),
            commands: Arc::default(),
            sections: Arc::default(),
            launch: launch::Launch::default(),
        }
    }
}

impl Control {
    pub fn set(&self, refusal: Refusal) {
        *self.refusal.lock().unwrap() = Some(refusal);
    }

    /// Every `(actor, operation)` `authorize` was asked about, in order.
    pub fn seen(&self) -> Vec<(Actor, Operation)> {
        self.seen.lock().unwrap().clone()
    }

    /// Every event `on_event` recorded, in order.
    pub fn events(&self) -> Vec<Value> {
        self.events.lock().unwrap().clone()
    }

    pub fn on_event(&self, behaviour: OnEvent) {
        *self.on_event.lock().unwrap() = behaviour;
    }

    /// Every `pdo ext test …` `run_cli` answered, in order.
    pub fn cli_calls(&self) -> Vec<CliInvocation> {
        self.cli_calls.lock().unwrap().clone()
    }

    /// The data folder `start` received.
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.lock().unwrap().clone()
    }

    /// Declare the bundle built for another major of React.
    pub fn set_incompatible(&self, incompatible: bool) {
        self.incompatible.store(incompatible, Ordering::Relaxed);
    }

    /// The contexts the bundle's command reported it was called with, in order.
    pub fn commands(&self) -> Vec<Value> {
        self.commands.lock().unwrap().clone()
    }

    /// What the bundle's sections reported their host saved, in order.
    pub fn sections(&self) -> Vec<Value> {
        self.sections.lock().unwrap().clone()
    }

    fn record_section(&self, saved: Value) {
        self.sections.lock().unwrap().push(saved);
    }

    fn record_command(&self, context: Value) {
        self.commands.lock().unwrap().push(context);
    }

    fn incompatible(&self) -> bool {
        self.incompatible.load(Ordering::Relaxed)
    }

    fn record(&self, event: &Value) {
        self.events.lock().unwrap().push(event.clone());
        if let Some(dir) = self.data_dir() {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("events.jsonl"))
            {
                let _ = writeln!(file, "{event}");
            }
        }
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
        let manifest = Manifest::new(ID, "Test extension", VERSION);
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

    fn start(&self, context: &ExtensionContext) {
        *self.control.data_dir.lock().unwrap() = Some(context.data_dir.clone());
    }

    fn node_env(&self, launch: &NodeLaunch) -> NodeEnv {
        self.control.launch.node_env(launch)
    }

    fn node_preamble(&self, launch: &NodeLaunch) -> Option<String> {
        self.control.launch.node_preamble(launch)
    }

    fn trigger_suspension(&self, trigger: &TriggerRef) -> Option<String> {
        self.control.launch.trigger_suspension(trigger)
    }

    fn on_create(&self, actor: &Actor, _entity: &EntityRef) -> Attributes {
        let started_by = match actor {
            Actor::Identified { id, .. } => id.clone(),
            other => other.kind().to_string(),
        };
        Attributes::from([("started_by".to_string(), started_by)])
    }

    fn on_event(&self, event: &Value) {
        let behaviour = *self.control.on_event.lock().unwrap();
        match behaviour {
            OnEvent::Record => {}
            OnEvent::Sleep(duration) => std::thread::sleep(duration),
            OnEvent::Panic => panic!("the test extension panics in on_event"),
        }
        self.control.record(event);
    }

    fn routes(&self) -> Option<pdo::extension_api::axum::Router> {
        Some(routes::router(self.control.clone()))
    }

    fn ui(&self) -> Option<UiContributions> {
        let major = if self.control.incompatible() {
            REACT_MAJOR - 1
        } else {
            REACT_MAJOR
        };
        let run = || ResourceKind::Run;
        // No icon: its button shows the fallback, a boxed « S ».
        let show_context = UiCommand::new(SHOW_CONTEXT, "Show context", "showContext");
        let never = UiCommand::new("test.never", "Never shown", "showContext");
        let mut ui = UiContributions::bundle(format!("/ext/{ID}/ui.js"), major)
            .with(UiSlot::new(
                Slot::Navigation,
                "events",
                "Test events",
                "EventsPage",
            ))
            .with(UiSlot::new(
                Slot::SettingsPanel,
                "settings",
                "Test extension",
                "SettingsPanel",
            ))
            .with(UiSlot::new(
                Slot::ListFilter { resource: run() },
                "started-by",
                "Started by",
                "StartedByFilter",
            ))
            .with(UiSlot::new(
                Slot::DetailTab { resource: run() },
                "run-tab",
                "Test",
                "RunTab",
            ))
            .contribute(
                UiContribution::page(UiLocation::TopbarEnd, "page", "Test page", "TestPage")
                    .with_icon(format!("/ext/{ID}/icon.svg")),
            )
            .contribute(
                UiContribution::action(UiLocation::TopbarEnd, "show-context", &show_context)
                    .when(When::has("run")),
            )
            .contribute(
                UiContribution::action(UiLocation::TopbarEnd, "never", &never)
                    .when(When::eq("run.status", "never-a-status")),
            )
            .command(show_context)
            .command(never)
            // #31: one element at each location the catalogue gained.
            .contribute(UiContribution::tab(
                UiLocation::NodeDetail,
                "node-tab",
                "Test",
                "EntityTab",
            ))
            .contribute(UiContribution::tab(
                UiLocation::TriggerDetail,
                "trigger-tab",
                "Test",
                "EntityTab",
            ))
            .contribute(UiContribution::tab(
                UiLocation::SkillDetail,
                "skill-tab",
                "Test",
                "EntityTab",
            ))
            .contribute(UiContribution::section(
                UiLocation::ProjectEdit,
                "project-section",
                "Test",
                "EntitySection",
            ))
            .contribute(UiContribution::section(
                UiLocation::RunNew,
                "new-run-section",
                "Test",
                "EntitySection",
            ))
            .contribute(UiContribution::section(
                UiLocation::NodeConfig,
                "node-section",
                "Test",
                "EntitySection",
            ))
            .contribute(UiContribution::section(
                UiLocation::PipelineConfig,
                "pipeline-section",
                "Test",
                "EntitySection",
            ))
            .contribute(UiContribution::section(
                UiLocation::RunInfo,
                "run-section",
                "Test",
                "EntitySection",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::RunItem,
                "run-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::TriggerItem,
                "trigger-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::PipelineItem,
                "pipeline-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::CanvasNode,
                "node-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::PortItem,
                "port-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::ReviewFile,
                "file-badge",
                "Test",
                "EntityBadge",
            ))
            .contribute(UiContribution::decoration(
                UiLocation::Statusbar,
                "status-badge",
                "Test",
                "EntityBadge",
            ))
            // Throws on every render: its error boundary keeps it to its own mark.
            .contribute(UiContribution::decoration(
                UiLocation::Statusbar,
                "exploding",
                "Exploding",
                "Exploding",
            ))
            .contribute(UiContribution::list_filter(
                UiLocation::TriggersFilter,
                "triggers-started-by",
                "Started by",
                "StartedByFilter",
            ))
            .contribute(UiContribution::list_filter(
                UiLocation::PipelinesFilter,
                "pipelines-started-by",
                "Started by",
                "StartedByFilter",
            ))
            .contribute(UiContribution::list_filter(
                UiLocation::ProjectsFilter,
                "projects-started-by",
                "Project started by",
                "StartedByFilter",
            ));
        for &location in ACTION_LOCATIONS {
            let id = action_id(location);
            let command = UiCommand::new(
                format!("test.{id}"),
                format!("Test {}", location.id()),
                "showContext",
            );
            let mut action = UiContribution::action(location, &id, &command);
            if location == UiLocation::RunItemMenu {
                action = action.when(When::not(When::eq("entity.status", "archived")));
            }
            ui = ui.contribute(action).command(command);
        }
        let fail = UiCommand::new(FAIL, "Fail on purpose", "failOnPurpose");
        Some(
            ui.contribute(UiContribution::action(
                UiLocation::RunItemMenu,
                "fail",
                &fail,
            ))
            .command(fail),
        )
    }

    fn cli(&self) -> Option<CliSpec> {
        Some(CliSpec::new(
            "Echo, cat, exit with a code: the test of `pdo ext`",
        ))
    }

    /// `echo <words…>` writes the words; `cat` its standard input; `exit
    /// <code>` exits with it; each writes the Actor on stderr. `panic`
    /// panics; anything else exits `2`.
    fn run_cli(&self, invocation: &CliInvocation) -> CliOutput {
        self.control
            .cli_calls
            .lock()
            .unwrap()
            .push(invocation.clone());
        let actor = format!("actor: {}\n", actor_label(&invocation.actor));
        let args: Vec<&str> = invocation.args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["echo", words @ ..] => {
                CliOutput::success(format!("{}\n", words.join(" "))).with_stderr(actor)
            }
            ["cat"] => CliOutput::success(invocation.stdin.clone()).with_stderr(actor),
            ["exit", code] => match code.parse() {
                Ok(code) => CliOutput::failure(code, actor),
                Err(_) => CliOutput::failure(2, format!("`{code}` is not an exit code\n")),
            },
            ["panic"] => panic!("the test extension panics in run_cli"),
            _ => CliOutput::failure(2, "unknown command\n"),
        }
    }

    fn skills(&self) -> Vec<ExtensionSkill> {
        let (path, content) = SKILL_REFERENCE;
        vec![ExtensionSkill::new(SKILL_NAME, SKILL_MD).with_file(path, content)]
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

/// A second test extension (`test2`): one section at `project/edit`, so the
/// location lists two extensions' sections, in registration order.
pub struct SecondTestExtension;

impl SecondTestExtension {
    /// Its bundle: one section, naming the Projet it edits.
    const BUNDLE: &'static str = r#"import React from "react";
export function SecondSection({ entity }) {
  return React.createElement("p", { "data-testid": "test2-section" },
    `Second extension · ${entity?.name || "(unnamed)"}`);
}
"#;
}

impl Extension for SecondTestExtension {
    fn manifest(&self) -> Manifest {
        Manifest::new(SECOND_ID, "Second test extension", "0.0.0")
    }

    fn routes(&self) -> Option<pdo::extension_api::axum::Router> {
        use pdo::extension_api::axum::http::header;
        use pdo::extension_api::axum::routing::get;
        Some(pdo::extension_api::axum::Router::new().route(
            "/ui.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], Self::BUNDLE) }),
        ))
    }

    fn ui(&self) -> Option<UiContributions> {
        Some(
            UiContributions::bundle(format!("/ext/{SECOND_ID}/ui.js"), REACT_MAJOR).contribute(
                UiContribution::section(
                    UiLocation::ProjectEdit,
                    "rights",
                    "Rights",
                    "SecondSection",
                ),
            ),
        )
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
    assert_eq!(
        Refusal::parse("a/item:create"),
        Ok(Refusal::Only(
            ResourceKind::extension("a", "item"),
            Verb::Create
        ))
    );
    assert!(Refusal::parse("nope:update").is_err());
    assert!(Refusal::parse("project").is_err());
    assert_eq!(
        reason(&Operation::new(ResourceKind::Project, Verb::Update)),
        "the test extension refuses update project"
    );
}
