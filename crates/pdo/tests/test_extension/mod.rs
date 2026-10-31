//! The internal test extension (ADR-0081, D17): the one guard that turns the core
//! red when it breaks its extension API. Internal to the test suite, never
//! published.
//!
//! - **`identify`**: the caller named by the [`ACTOR_HEADER`] request header
//!   (or by the cookie `pdo-test-actor`, `run_view.rs`) becomes
//!   `Actor::Identified { namespace: "test", id }`; so does the holder
//!   of the token it may require (`gate.rs`, [`Control::set_gate`]); otherwise
//!   the core decides.
//! - **`admit`**: with a token required, refuses a request nobody identified,
//!   or redirects it to its sign-in page (`gate.rs`).
//! - **`display_name`**: names the Actors it identifies (`alice` → `Alice`).
//! - **`authorize`**: refuses what its [`Refusal`] says, with a reason naming the
//!   Operation. The refusal can change while the daemon runs ([`Control::set`],
//!   `POST /ext/test/refusal`), so a test seeds its fixtures before refusing
//!   everything. CORE-5: it refuses a read (`run:read`, `project:read`,
//!   `repository:read` for the disk) when a rule says so, possibly only in
//!   one Projet (`@<project id>`) and for one caller (`~<actor>`); `all`
//!   refuses every mutation and never a read.
//! - Every `(Actor, Operation)` it is asked about is recorded ([`Control::seen`]),
//!   with the Projets the Operation is in; `GET /ext/test/operations` lists
//!   them.
//! - **`manifest`** declares one Ressource, [`NOTE`] (`test/note`), so the
//!   catalogue (`GET /resources`) lists it after the core's.
//! - **`on_create`**: stamps `started_by` (so `test/started_by`): the id of an
//!   identified caller, else the kind of actor (`local_operator`, `trigger`…).
//! - **`on_event`**: records every event ([`Control::events`]) and appends it
//!   to `events.jsonl` in its data folder; it can be told to sleep or panic
//!   first ([`OnEvent`]).
//! - **`start`**: remembers its data folder ([`Control::data_dir`]).
//! - **`routes`** (`routes.rs`): `/ext/test` answers who it is, `/events` the
//!   trace of the events, `/operations` the Operations it was asked about, `/refusal` what it refuses (`POST {"refuse": "<text form>"}` changes it), `/ui.js` its UI bundle, `/icon.svg` its icon,
//!   `/commands` the trace of its command's calls (`POST` records one),
//!   `/incompatible` switches the bundle to a React major the front does not run.
//! - **`ui`** (`ui.js`): one element at each location of the catalogue. The
//!   four locations of 0.1 through their 0.1 slots (`Navigation`,
//!   `SettingsPanel`, `ListFilter`/`DetailTab` of the Runs), so they prove the
//!   0.1 form still renders; `topbar/end` through 0.2 contributions: a page with
//!   its icon (which opens `/ws` through `openProbedSocket` of
//!   `@pdo/extension-ui`, 0.10, and shows the socket's state), a panel under
//!   its icon (`MessagesPanel`, 0.20: the items of `messages.rs`, each deleted with
//!   an « Undo » in a toast; the icon counts them), an action calling [`SHOW_CONTEXT`] while a Run is selected, and
//!   an action whose `when` never holds. The page, at `screens/alpha` and
//!   `screens/beta`, sets the title and the icon of the shell's header per
//!   route; `screens/beta` shows a read-only Banner with a lock (0.20); its
//!   body shows a text kept in a yjs document, the front's own yjs (0.20). Every Action location (#30) through
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
//! - `topbar/account` (0.8): `AccountMenu`, an avatar opening a menu that
//!   names the signed-in Actor through `useCurrentActor` of `@pdo/extension-ui`.
//! - `stats/tab` (0.9): `StatsTab`, which shows the period and the Refresh
//!   count it is handed, and `Exploding`, a tab that throws; `settings/page`
//!   (0.9): `SettingsPage`, which shows its deep link's path.
//! - `runs/empty` (0.11): `RunsEmptyNote`, a line saying why the Runs list is
//!   empty, and a Copy button through `copyToClipboard` of `@pdo/extension-ui`.
//! - `review/composer` (0.14): `tags`, a source of completion opened by `#`
//!   (`suggestTags`: `bug`, `perf`, `review-later` and `run-<the Run's id>`),
//!   whose inserted `#tag` renders as a chip (`TagChip`).
//! - `palette` (0.18): `new-note`, a global action calling [`NEW_NOTE`], which
//!   makes `test/note` × `create` (refuse it to hide the action), and `notes`,
//!   a Source de recherche (`searchNotes`: the notes whose title holds the
//!   query, each a deep link to the test page); `members` (0.20), another
//!   (`searchPeople`), whose results carry an avatar.
//! - `palette` (0.20, `palette.rs`): its two contributions rank its group 2;
//!   its Source answers a note per status shape (`todo`, `doing`, `review`,
//!   `done`) and one with a 200-character title without a space; `inbox`, a
//!   global action calling [`palette::INBOX`], shows the counter `GET
//!   /ext/test/inbox` answers ([`palette::START`] until `PUT
//!   /ext/test/inbox` sets it, which publishes `inbox_changed`).
//! - **`skills`**: one skill, `test-extension-cli` (`skill/`), at the
//!   extension's version.
//! - **`node_env`** / **`node_preamble`** / **`trigger_suspension`** (API
//!   0.8, `launch.rs`): the variables, the preamble and the suspensions the
//!   test sets ([`Control::launch`]).
//! - **`session_env`** / **`session_preamble`** (API 0.13, `launch.rs`): for a
//!   Node, its `node_env` / `node_preamble`; for the Pipeline Manager and the
//!   library assistant, the same variables plus `TEST_STARTED_BY` (the id of
//!   who starts the session, else its kind of Actor), and the same preamble.
//!   The session events (`session_started`, `session_taken_over`,
//!   `session_ended`) reach `on_event` like every other event.
//! - `projets.rs` (API 0.11): the name of a Projet through its `Host`, the
//!   signal of the Triggers, and `PUT /ext/test/notes/<id>`, which publishes
//!   a `note_changed` guarded by the read of the note in its Projet.
//! - **`validate_create`** / **`on_create_with_data`** (API 0.9,
//!   `creation.rs`): refuses a value holding a string starting with `refuse`,
//!   records each creation and the order of the stamps and the launches,
//!   stamps `data.<key>`; `POST /ext/test/runs` creates a Run through its
//!   `Host` ([`Control::creation`]).
//! - **`Host::project_name`** / **`Host::triggers_changed`** (API 0.11,
//!   `projets.rs`): `GET /ext/test/projects/<id>/name` answers the name the
//!   core gives, `POST /ext/test/triggers-changed` signals the Triggers; a
//!   suspension set or lifted (`/ext/test/suspensions`) signals them too.
//! - **`Host::presences`** / **`Host::run_cost`** (API 0.12, `run_view.rs`):
//!   `GET /ext/test/runs/<id>/view` answers them; `GET /ext/test/act-as?actor=
//!   <id>` signs a browser in as that Actor; `PeopleMarks`, at
//!   `run/item/people`, stacks the avatars of the Presences of the row's Run.
//! - **`Host::open_session`** and the other gestures of the Sessions
//!   hébergées (API 0.19, `sessions.rs`): `POST /ext/test/sessions` opens one
//!   on behalf of the caller, `GET` lists or reads them, `DELETE
//!   /ext/test/sessions/<key>` terminates one, `POST …/<key>/relaunch`, `PUT
//!   …/<key>/config`, `PUT …/<key>/zoom`, `POST …/<key>/type` and `POST
//!   …/<key>/forget` relaunch, configure, zoom, type into and forget it, `GET
//!   …/<key>/live[/<path>]` lists and reads its Artefacts live. Its page, at
//!   the path `hosted` (`#/ext/test/topbar/end/hosted`), opens the shell
//!   `demo` (`hosted/agent`: the agent `demo-agent`) and shows it with the
//!   core's exported terminal, and its detail composed of the core's bricks
//!   in a 400 px drawer (API 0.19). The page's icon carries a counter (`GET
//!   /ext/test/counter`): the live sessions the caller started, accented
//!   while one waits. A Session hébergée gets a `PDO_TOKEN`
//!   naming who started it, which `identify` recognises, so a `pdo` typed in
//!   a hosted shell acts as that Actor.
//! - [`SecondTestExtension`] (`test2`): a second extension with a section at
//!   `project/edit`, a tab at `session/detail` and a global action of
//!   `palette` ranked 1, so two extensions share a location, and its group
//!   comes before the test extension's in the command field although it is
//!   registered after it.
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

pub mod creation;
pub mod gate;
pub mod hosted;
pub mod interop;
pub mod launch;
pub mod messages;
pub mod palette;
pub mod projets;
mod routes;
pub mod run_view;
pub mod sessions;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdo::extension_api::{
    Actor, ActorDisplay, Admission, Attributes, Authorization, CliInvocation, CliOutput, CliSpec,
    EntityRef, Extension, ExtensionContext, ExtensionSkill, Manifest, NodeEnv, NodeLaunch,
    Operation, RequestParts, ResourceDeclaration, ResourceKind, SessionLaunch, Slot, TriggerRef,
    UiCommand, UiContribution, UiContributions, UiLocation, UiSlot, Verb, When,
};
use serde_json::Value;

/// The id of the test extension (its namespace for the actors it identifies,
/// the prefix of its attributes, its routes under `/ext/test/`).
pub const ID: &str = "test";

/// The major of React the test bundle declares; the incompatible one declares
/// the one before.
pub const REACT_MAJOR: u32 = 19;

/// The kind of the one Ressource the test extension declares (`test/note`).
pub const NOTE: &str = "note";

/// What the test extension declares of [`NOTE`]: its label and its verbs.
pub fn note() -> ResourceDeclaration {
    ResourceDeclaration::new(NOTE, "Test notes", ["create", "update", "delete", "pin"])
        .described("The notes of the test extension")
}

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

/// The id of the test extension's global action's command (`palette`, 0.18):
/// it reports its context as [`SHOW_CONTEXT`] does, and makes `test/note` ×
/// `create`, so a refusal of that Operation hides it.
pub const NEW_NOTE: &str = "test.new-note";

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
    /// Every mutation; never a read ([`Verb::Read`]), so the lists still show
    /// what a test seeded.
    Everything,
    /// One verb on one kind of resource.
    Only(ResourceKind, Verb),
    /// Each Operation one of the rules matches (CORE-5: a read in one Projet,
    /// for one caller…).
    Rules(Vec<Rule>),
    /// Panics in `authorize`, on every mutation (never on a read).
    Panic,
}

/// One rule of [`Refusal::Rules`]: a verb on a kind of resource, only in one
/// Projet and only for one identified caller when given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub resource: ResourceKind,
    pub verb: Verb,
    /// Only an Operation in this Projet (its id); `""`: only an Operation in
    /// no Projet.
    pub project: Option<String>,
    /// Only for the caller identified by this id (the [`ACTOR_HEADER`]).
    pub actor: Option<String>,
}

impl Rule {
    /// `<resource>:<verb>[@<project>][~<actor>]`.
    fn parse(text: &str) -> Result<Self, String> {
        let (rest, actor) = match text.split_once('~') {
            Some((rest, actor)) => (rest, Some(actor.trim().to_string())),
            None => (text, None),
        };
        let (rest, project) = match rest.split_once('@') {
            Some((rest, project)) => (rest, Some(project.trim().to_string())),
            None => (rest, None),
        };
        let (resource, verb) = rest.trim().split_once(':').ok_or_else(|| {
            format!("`{text}`: expected none, all, panic or resource:verb[@project][~actor]")
        })?;
        let resource = ResourceKind::from_wire_name(resource)
            .ok_or_else(|| format!("unknown resource `{resource}`"))?;
        let verb = match verb {
            "read" => Verb::Read,
            "create" => Verb::Create,
            "update" => Verb::Update,
            "delete" => Verb::Delete,
            action => Verb::Action(action.to_string()),
        };
        Ok(Rule {
            resource,
            verb,
            project,
            actor,
        })
    }

    fn matches(&self, actor: &Actor, operation: &Operation) -> bool {
        operation.resource == self.resource
            && operation.verb == self.verb
            && self.project.as_ref().is_none_or(|project| {
                if project.is_empty() {
                    operation.is_outside_projects()
                } else {
                    operation.projects.contains(project)
                }
            })
            && self.actor.as_ref().is_none_or(|wanted| {
                matches!(actor, Actor::Identified { namespace, id } if namespace == ID && id == wanted)
            })
    }
}

impl Refusal {
    /// Read a refusal from its text form: `none`, `all`, `panic`, or rules
    /// separated by `,`, each `<resource>:<verb>` with the wire names of both
    /// (`project:update`, `trigger:fire` for the action `fire`, `run:read`,
    /// `a/item:create` for an extension's resource), then optionally
    /// `@<project id>` (only in that Projet; a bare `@`: only in no Projet)
    /// and `~<actor>` (only for that identified caller):
    /// `run:read@p2,repository:read,run:create@,run:create@p2~bob`.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.trim() {
            "none" => Ok(Refusal::Nothing),
            "all" => Ok(Refusal::Everything),
            "panic" => Ok(Refusal::Panic),
            other => {
                let rules = other
                    .split(',')
                    .filter(|rule| !rule.trim().is_empty())
                    .map(Rule::parse)
                    .collect::<Result<Vec<_>, _>>()?;
                match rules.as_slice() {
                    [] => Err("no rule".to_string()),
                    [Rule {
                        resource,
                        verb,
                        project: None,
                        actor: None,
                    }] => Ok(Refusal::Only(resource.clone(), verb.clone())),
                    _ => Ok(Refusal::Rules(rules)),
                }
            }
        }
    }

    fn refuses(&self, actor: &Actor, operation: &Operation) -> bool {
        match self {
            Refusal::Nothing => false,
            Refusal::Everything | Refusal::Panic => !operation.is_read(),
            Refusal::Only(resource, verb) => {
                operation.resource == *resource && operation.verb == *verb
            }
            Refusal::Rules(rules) => rules.iter().any(|rule| rule.matches(actor, operation)),
        }
    }
}

/// The reason the test extension gives for refusing `operation`.
pub fn reason(operation: &Operation) -> String {
    format!(
        "the test extension refuses {} {}",
        operation.verb.name(),
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
    gate: gate::SharedGate,
    /// Its launch hooks and its suspensions of Triggers (API 0.8).
    pub launch: launch::Launch,
    /// What it saw of the creations, and its Host (API 0.9).
    pub creation: creation::Creation,
    /// The count of its `Inbox` Action (API 0.20).
    pub inbox: palette::Inbox,
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
            gate: Arc::default(),
            launch: launch::Launch::default(),
            creation: creation::Creation::default(),
            inbox: palette::Inbox::default(),
        }
    }
}

impl Control {
    /// What the extension requires to admit a request (a token, or nothing).
    pub fn set_gate(&self, gate: gate::Gate) {
        *self.gate.lock().unwrap() = gate;
    }

    pub fn gate(&self) -> gate::Gate {
        self.gate.lock().unwrap().clone()
    }

    pub fn set(&self, refusal: Refusal) {
        *self.refusal.lock().unwrap() = Some(refusal);
    }

    /// What it refuses now.
    pub fn refusal(&self) -> Refusal {
        self.refusal
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Refusal::Nothing)
    }

    /// Every `(actor, operation)` `authorize` was asked about for a
    /// mutation, in order: the reads (CORE-5) left out, so a test of the
    /// mutations reads the same sequence whatever the UI or the test read.
    pub fn seen(&self) -> Vec<(Actor, Operation)> {
        self.asked()
            .into_iter()
            .filter(|(_, operation)| !operation.is_read())
            .collect()
    }

    /// Every `(actor, operation)` `authorize` was asked about, the reads
    /// included, in order (what `GET /ext/test/operations` lists).
    pub fn asked(&self) -> Vec<(Actor, Operation)> {
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
        let manifest = Manifest::new(ID, "Test extension", VERSION).with_resource(note());
        match &self.api_version {
            Some(version) => manifest.requiring_api(version.clone()),
            None => manifest,
        }
    }

    fn identify(&self, request: &RequestParts) -> Option<Actor> {
        let named = request
            .headers
            .get(ACTOR_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|caller| !caller.is_empty())
            .map(str::to_string)
            .or_else(|| run_view::actor_of_cookie(request))
            .map(|caller| Actor::Identified {
                namespace: ID.to_string(),
                id: caller,
            });
        named
            .or_else(|| sessions::identify(request))
            .or_else(|| gate::identify(&self.control.gate(), request))
    }

    fn admit(&self, request: &RequestParts) -> Admission {
        gate::admit(&self.control.gate(), request)
    }

    fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
        gate::display_name(actor)
    }

    fn start(&self, context: &ExtensionContext) {
        *self.control.data_dir.lock().unwrap() = Some(context.data_dir.clone());
        self.control.creation.keep_host(context.host.clone());
    }

    fn node_env(&self, launch: &NodeLaunch) -> NodeEnv {
        self.control.creation.launched(launch);
        self.control.launch.node_env(launch)
    }

    fn node_preamble(&self, launch: &NodeLaunch) -> Option<String> {
        self.control.launch.node_preamble(launch)
    }

    fn session_env(&self, launch: &SessionLaunch) -> NodeEnv {
        match &launch.node {
            Some(node) => self.node_env(node),
            None => self.control.launch.session_env(launch),
        }
    }

    fn session_preamble(&self, launch: &SessionLaunch) -> Option<String> {
        match &launch.node {
            Some(node) => self.node_preamble(node),
            None => self.control.launch.session_preamble(launch),
        }
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

    fn validate_create(
        &self,
        _actor: &Actor,
        kind: &ResourceKind,
        value: &Value,
    ) -> Result<(), String> {
        if !creation::is_run(kind) {
            return Err(format!(
                "the test extension takes data for a Run only, not {kind}"
            ));
        }
        self.control.creation.validate(value)
    }

    fn on_create_with_data(
        &self,
        actor: &Actor,
        entity: &EntityRef,
        data: Option<&Value>,
    ) -> Attributes {
        let mut attributes = self.on_create(actor, entity);
        if creation::is_run(&entity.kind) {
            attributes.extend(self.control.creation.stamped(actor, entity, data));
        }
        attributes
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
            .importing_ui_module()
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
                    .with_icon(format!("/ext/{ID}/icon.svg"))
                    .with_counter(format!("/ext/{ID}/counter")),
            )
            // 0.20: a panel under its icon, its counter the messages.
            .contribute(
                UiContribution::panel(
                    UiLocation::TopbarEnd,
                    "messages",
                    "Test messages",
                    "MessagesPanel",
                )
                .with_icon(format!("/ext/{ID}/messages.svg"))
                .with_counter(format!("/ext/{ID}/messages/counter")),
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
            .contribute(UiContribution::tab(
                UiLocation::SessionDetail,
                "session-tab",
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
            // 0.8: the avatar of the signed-in Actor, after Settings.
            .contribute(UiContribution::decoration(
                UiLocation::TopbarAccount,
                "account",
                "Account",
                "AccountMenu",
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
            ))
            // 0.9: a tab of Stats after Performance, one that throws (its
            // boundary keeps it to its own tab), and a page of Settings.
            .contribute(UiContribution::tab(
                UiLocation::StatsTab,
                "stats-tab",
                "Test stats",
                "StatsTab",
            ))
            .contribute(UiContribution::tab(
                UiLocation::StatsTab,
                "stats-exploding",
                "Exploding",
                "Exploding",
            ))
            .contribute(UiContribution::page(
                UiLocation::SettingsPage,
                "settings-page",
                "Test settings",
                "SettingsPage",
            ))
            // 0.11: an explanation in the empty Runs list.
            .contribute(UiContribution::decoration(
                UiLocation::RunsEmpty,
                "runs-empty",
                "Why empty",
                "RunsEmptyNote",
            ))
            // 0.12: who has the row's Run open.
            .contribute(UiContribution::decoration(
                UiLocation::RunItemPeople,
                "people",
                "People",
                "PeopleMarks",
            ))
            // 0.14: a source of completion of the Review comment editor.
            .contribute(
                UiContribution::completion(
                    UiLocation::ReviewComposer,
                    "tags",
                    "Tags",
                    '#',
                    "suggestTags",
                )
                .rendering("#[a-z][a-z0-9-]*", "TagChip"),
            );
        // 0.18: the command field, a global action guarded by the Operation it
        // makes, and a search source. 0.20: the group ranked 2, and a global
        // action with a counter.
        let new_note = UiCommand::new(NEW_NOTE, "New note", "showContext")
            .guarded_by(ResourceKind::extension(ID, NOTE), Verb::Create);
        let inbox = UiCommand::new(palette::INBOX, "Inbox", "showContext");
        ui = ui
            .contribute(
                UiContribution::action(UiLocation::Palette, "new-note", &new_note).with_rank(2),
            )
            .command(new_note)
            .contribute(
                UiContribution::search(UiLocation::Palette, "notes", "Notes", "searchNotes")
                    .with_icon(format!("/ext/{ID}/icon.svg"))
                    .with_rank(2),
            )
            // 0.20: results carrying an avatar.
            .contribute(UiContribution::search(
                UiLocation::Palette,
                "members",
                "Members",
                "searchPeople",
            ))
            .contribute(
                UiContribution::action(UiLocation::Palette, "inbox", &inbox)
                    .with_counter(format!("/ext/{ID}/inbox")),
            )
            .command(inbox);
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
        if refusal == Refusal::Panic && !operation.is_read() {
            panic!("the test extension panics in authorize");
        }
        if refusal.refuses(actor, operation) {
            Authorization::deny(reason(operation))
        } else {
            Authorization::Allow
        }
    }
}

/// A second test extension (`test2`): one section at `project/edit`, so the
/// location lists two extensions' sections, in registration order, and one
/// tab at `session/detail` (API 0.19), so the detail of a Session hébergée the
/// test extension composes shows another extension's tab; one global action
/// of `palette` ranked 1 (API 0.20), so its group comes before the test
/// extension's (ranked 2) although it is registered after it.
pub struct SecondTestExtension;

impl SecondTestExtension {
    /// The id of its global action's command (`palette`, 0.20).
    pub const ACTION: &'static str = "test2.hello";

    /// Its bundle: one section, naming the Projet it edits; one tab, naming
    /// the session it details; the function of its global action.
    const BUNDLE: &'static str = r#"import React from "react";
export function SecondSection({ entity }) {
  return React.createElement("p", { "data-testid": "test2-section" },
    `Second extension · ${entity?.name || "(unnamed)"}`);
}
export function SessionTab({ entity, context }) {
  const session = context.session;
  return React.createElement("p", { "data-testid": "test2-session-tab" },
    `Second extension · ${entity.kind} ${session?.key ?? entity.id} of ${session?.extension ?? "?"} · ${session?.state ?? "?"}`);
}
export function hello() {}
"#;

    fn hello() -> UiCommand {
        UiCommand::new(Self::ACTION, "Second hello", "hello")
    }
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
            UiContributions::bundle(format!("/ext/{SECOND_ID}/ui.js"), REACT_MAJOR)
                .contribute(UiContribution::section(
                    UiLocation::ProjectEdit,
                    "rights",
                    "Rights",
                    "SecondSection",
                ))
                .contribute(UiContribution::tab(
                    UiLocation::SessionDetail,
                    "session",
                    "Second",
                    "SessionTab",
                ))
                .contribute(
                    UiContribution::action(UiLocation::Palette, "hello", &Self::hello())
                        .with_rank(1),
                )
                .command(Self::hello()),
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
    assert_eq!(
        Refusal::parse("run:read@p2, repository:read,run:create@p2~bob"),
        Ok(Refusal::Rules(vec![
            Rule {
                resource: ResourceKind::Run,
                verb: Verb::Read,
                project: Some("p2".into()),
                actor: None,
            },
            Rule {
                resource: ResourceKind::Repository,
                verb: Verb::Read,
                project: None,
                actor: None,
            },
            Rule {
                resource: ResourceKind::Run,
                verb: Verb::Create,
                project: Some("p2".into()),
                actor: Some("bob".into()),
            },
        ]))
    );
    let outside = Refusal::parse("run:create@,repository:read").unwrap();
    let create = Operation::new(ResourceKind::Run, Verb::Create);
    let actor = Actor::LocalOperator;
    assert!(outside.refuses(&actor, &create));
    assert!(!outside.refuses(&actor, &create.clone().in_projects(["p1"])));
    let bobs = Refusal::parse("run:read@p2~bob,project:read").unwrap();
    let read = Operation::new(ResourceKind::Run, Verb::Read).in_projects(["p2"]);
    let bob = Actor::Identified {
        namespace: ID.into(),
        id: "bob".into(),
    };
    assert!(bobs.refuses(&bob, &read));
    assert!(!bobs.refuses(&actor, &read));
    assert!(
        !Refusal::Everything.refuses(&actor, &read),
        "all never refuses a read"
    );
    assert_eq!(
        Refusal::parse("run:read"),
        Ok(Refusal::Only(ResourceKind::Run, Verb::Read))
    );
    assert!(Refusal::parse("nope:update").is_err());
    assert!(Refusal::parse("project").is_err());
    assert_eq!(
        reason(&Operation::new(ResourceKind::Project, Verb::Update)),
        "the test extension refuses update project"
    );
}
