use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    Actor, ActorDisplay, Admission, Attributes, Authorization, CliInvocation, CliOutput, CliSpec,
    EntityRef, ExtensionSkill, Host, Manifest, NeutralHost, NodeEnv, NodeLaunch, Operation,
    ResourceKind, SessionLaunch, TriggerRef, UiContributions,
};

/// The head of an HTTP request, as `identify` sees it: method, URI, headers.
/// Never the body, which a hook must not consume.
pub type RequestParts = axum::http::request::Parts;

/// What the core gives an extension when the daemon starts.
#[derive(Clone)]
#[non_exhaustive]
pub struct ExtensionContext {
    /// The extension's own data folder, `~/.pdo/extensions/<id>/`, created
    /// before [`Extension::start`] is called: its database, its files. Nothing
    /// else of the core writes there.
    pub data_dir: PathBuf,
    /// The core's mechanisms, for the extension's own mutations: the
    /// Autorisation d'opération of every extension, the events, the
    /// attributes of the other extensions. Keep it: it serves as long as the
    /// daemon runs.
    pub host: Arc<dyn Host>,
}

impl ExtensionContext {
    /// The context of the data folder `data_dir`, with the [`NeutralHost`]
    /// (everything allowed, events go nowhere, no attribute).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            host: Arc::new(NeutralHost),
        }
    }

    /// The same context, with `host`.
    pub fn with_host(mut self, host: Arc<dyn Host>) -> Self {
        self.host = host;
        self
    }
}

impl fmt::Debug for ExtensionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionContext")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

/// Two contexts are equal when they name the same folder and share the same
/// host.
impl PartialEq for ExtensionContext {
    fn eq(&self, other: &Self) -> bool {
        self.data_dir == other.data_dir
            && std::ptr::addr_eq(Arc::as_ptr(&self.host), Arc::as_ptr(&other.host))
    }
}

impl Eq for ExtensionContext {}

/// A third-party module registered when the daemon starts (ADR-0081).
///
/// Only [`Extension::manifest`] is required. Every hook defaults to the neutral
/// answer: an extension that overrides nothing changes nothing.
///
/// Hooks are called from the daemon's request path, so they must be quick. A
/// panic in `authorize` is a refusal; `on_event` is called off the request path
/// and can never slow the core down.
///
/// When the daemon starts, it calls [`Extension::skills`] once (to seed the
/// Banque de skills, before `start`), then [`Extension::start`] once, then
/// [`Extension::routes`] and [`Extension::ui`], before it serves anything.
pub trait Extension: Send + Sync + 'static {
    /// Who this extension is.
    fn manifest(&self) -> Manifest;

    /// Receive what the core gives the extension (its data folder, its
    /// [`Host`]), once, when the daemon starts. Does nothing by default.
    fn start(&self, _context: &ExtensionContext) {}

    /// Identify the actor behind a request. `None` (the default) lets the next
    /// extension, then the core, decide: the local operator, or the agent of the
    /// node session the request comes from.
    fn identify(&self, _request: &RequestParts) -> Option<Actor> {
        None
    }

    /// What becomes of a request **no extension identified** ([`Self::identify`]
    /// answered `None` everywhere): served ([`Admission::Admit`], the default),
    /// refused with `401` ([`Admission::Refuse`]), or refused with `401` naming
    /// the page to sign in on ([`Admission::Redirect`]), which the core's
    /// interface follows. Every extension is asked, in registration order; the
    /// first that does not admit wins. A panic refuses.
    ///
    /// Asked for every route of the API, `/ws`, the terminals' PTY and every
    /// extension's routes included (the extension's own sign-in page and the
    /// files it needs: admit them by their path), never for the SPA's own
    /// files. Called on the request path: be quick.
    fn admit(&self, _request: &RequestParts) -> Admission {
        Admission::Admit
    }

    /// How `actor` shows to a person: its name and avatar, when this
    /// extension knows it (typically the Actors it identifies). `None` (the
    /// default) lets the next extension answer; with no answer, the core shows
    /// the Actor as it always did. Called on the request path: be quick.
    fn display_name(&self, _actor: &Actor) -> Option<ActorDisplay> {
        None
    }

    /// Allow or refuse an Operation before it runs. Allows by default. Never
    /// consulted for the scheduler's internal transitions: an extension cannot
    /// freeze a Run in flight.
    ///
    /// Also asked about the reads ([`Verb::Read`](crate::Verb::Read), CORE-5):
    /// once per Run a list holds, per Run event `/ws` relays to a connection
    /// (the answer kept a few seconds), per Projet listed, per disk browse,
    /// and for the question of authorization (`POST /authorize`), which asks
    /// without running anything. A refused read hides the entity from a list
    /// and from `/ws`, and answers `403` for one entity: refuse a read on its
    /// resource and verb, never on a verb you do not know.
    fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
        Authorization::Allow
    }

    /// The attributes to stamp on an entity the actor just created: a Run, a
    /// Projet, an agentic profile, a Trigger, a pipeline of the library. The
    /// core prefixes each key with the extension's id (`<id>/<key>`), stores
    /// them outside the event log, returns them with the entity and filters
    /// the entity lists on them (`?attr.<id>/<key>=<value>`). None by default.
    ///
    /// Also called for the entities another extension creates
    /// ([`ResourceKind::Extension`](crate::ResourceKind::Extension), through
    /// its [`Host::on_create`]): that extension stores the prefixed attributes
    /// and returns them with its entities.
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }

    /// Check the value a creation carries for this extension (API 0.9), before
    /// anything is created: `POST /runs` `{"extensions": {"<id>": <value>}}`,
    /// `pdo run create --ext <id>.<key>=<value>`, a `run/new` section that
    /// provides one, a [`Host::create_run`](crate::Host::create_run). `kind`
    /// is what is being created ([`ResourceKind::Run`]).
    ///
    /// Asked only when the creation carries a value for this extension, once
    /// the Autorisation d'opération allowed it. `Err(reason)` refuses the
    /// creation: `400` with the reason, nothing created. Accepts everything by
    /// default. Called on the request path: be quick. A panic refuses.
    fn validate_create(
        &self,
        _actor: &Actor,
        _kind: &ResourceKind,
        _value: &serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }

    /// [`Self::on_create`], with the value the creation carried for this
    /// extension (API 0.9): `None` when it carried none, and for every entity
    /// but a Run. Called for a new Run **before its first Node is launched**,
    /// the value already checked by [`Self::validate_create`].
    ///
    /// By default, [`Self::on_create`]: override this one to read the value,
    /// that one otherwise.
    fn on_create_with_data(
        &self,
        actor: &Actor,
        entity: &EntityRef,
        _data: Option<&serde_json::Value>,
    ) -> Attributes {
        self.on_create(actor, entity)
    }

    /// Observe an event: exactly the JSON the daemon broadcasts on `/ws`, which
    /// carries an [`OperationApplied`](crate::OperationApplied) after each
    /// authorized and successful mutation, and the events the other extensions
    /// publish ([`ExtensionEvent`](crate::ExtensionEvent); never the
    /// extension's own). Ignores everything by default.
    ///
    /// Called on the extension's own thread, in the order the events came: a
    /// slow `on_event` delays only this extension's next events, never the
    /// core nor another extension. A panic is logged and the next event is
    /// delivered as usual.
    fn on_event(&self, _event: &serde_json::Value) {}

    /// Routes served under `/ext/<id>/`: the router is nested there, so it
    /// declares its paths from `/` (its `/` answers `/ext/<id>`, its `/events`
    /// `/ext/<id>/events`). It can never hide a route of the core. None by
    /// default.
    ///
    /// The whole `/ext/` prefix is the API's, never the SPA's, whatever the
    /// method: a path the router does not serve answers `404`
    /// `{"error": "not_found", "message", "id"}`; without routes (`None`, or a
    /// panic here), and for any id no loaded extension has, `/ext/<id>` and
    /// everything under it answer `404`
    /// `{"error": "extension_not_found", "message", "id"}`.
    fn routes(&self) -> Option<axum::Router> {
        None
    }

    /// Contributions to the interface. None by default.
    fn ui(&self) -> Option<UiContributions> {
        None
    }

    /// The command line of the extension, `pdo ext <id> <args…>`. `None` (the
    /// default): no command line, and `pdo ext <id>` exits
    /// [`EXIT_NO_CLI`](crate::EXIT_NO_CLI).
    fn cli(&self) -> Option<CliSpec> {
        None
    }

    /// Run one `pdo ext <id> <args…>`, when [`Extension::cli`] declares a
    /// command line. The CLI of the core relays the arguments, the standard
    /// input and the caller's Actor (the agent of a node session, the local
    /// operator otherwise), then writes the answer's stdout and stderr byte
    /// for byte and exits with its code.
    ///
    /// Called on a blocking thread of the daemon's tokio runtime, never on its
    /// request path: it may take its time, and drive an async call with
    /// `tokio::runtime::Handle::current().block_on`. A panic answers exit code
    /// `1`, like a daemon that gives no verdict. By default, exit `1` with a
    /// message: declaring a command line without running it is a bug.
    fn run_cli(&self, _invocation: &CliInvocation) -> CliOutput {
        CliOutput::failure(
            1,
            "this extension declares a command line but does not run it\n",
        )
    }

    /// The skills this extension brings, seeded into the Banque de skills
    /// once, when the daemon starts: called **before** [`Extension::start`],
    /// so the list must not depend on it (embed the files). A skill the list
    /// no longer holds is removed from the bank. None by default.
    fn skills(&self) -> Vec<ExtensionSkill> {
        Vec::new()
    }

    /// The environment variables to add to the Node being launched, an agent
    /// or a script, on the host or in its Run's sandbox. None by default.
    ///
    /// The core adds them to the Node's environment, but **never overwrites
    /// one of its own**: a name the core sets for this Node (`PDO_RUN_ID`,
    /// `PATH`, `HOME`, the harness's variables, a script's `PDO_INPUT_*`…) is
    /// dropped with a warning. Between two extensions, the first registered
    /// wins.
    ///
    /// When a variable names `CLAUDE_CONFIG_DIR`, the core reads that Node's
    /// Claude Code transcripts (its cost, its liveness) under that folder
    /// instead of `~/.claude`.
    ///
    /// The git identity variables (`GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`,
    /// `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL`) also sign the Node's
    /// delivery: its `<node> iter-<N>: completed` commit and, for an isolated
    /// Node, the merge of its sub-worktree into the Run's and the resolution
    /// commit in the Node's favour. The core freezes these four, and nothing
    /// else of the environment, on the Node's start. Give all four for a
    /// daemon with no git identity of its own to deliver.
    ///
    /// Called on the daemon's spawn path, once per launch: keep it quick. A
    /// panic adds nothing.
    fn node_env(&self, _launch: &NodeLaunch) -> NodeEnv {
        NodeEnv::new()
    }

    /// A text the core places at the very top of an agent Node's PDO preamble,
    /// before `# PDO Runtime Preamble`. Never asked for a script (it reads no
    /// preamble). None by default.
    ///
    /// Two extensions' texts follow each other in registration order. Called
    /// on the daemon's spawn path: keep it quick. A panic adds nothing.
    fn node_preamble(&self, _launch: &NodeLaunch) -> Option<String> {
        None
    }

    /// The environment variables to add to any session being launched that
    /// makes an agent act, or runs a script Node (API 0.13): a Node, the
    /// Pipeline Manager of a Run, the library assistant. The core asks this
    /// hook, never [`Self::node_env`] directly: by default it answers
    /// [`Self::node_env`] for a Node and nothing for the other sessions, so an
    /// extension that only knows `node_env` behaves as before.
    ///
    /// The rules of [`Self::node_env`] hold for every session: the core never
    /// overwrites one of its own variables, the first extension registered
    /// wins a name, a panic adds nothing. The launch names the session, its
    /// Run and Projets, and the Actor who starts it
    /// ([`SessionLaunch::started_by`]); the end of the session is published
    /// as a [`SessionEvent`](crate::SessionEvent) (`session_ended`), so what
    /// the variables hand over can be revoked then. Called on the daemon's
    /// spawn path, once per launch: keep it quick.
    fn session_env(&self, launch: &SessionLaunch) -> NodeEnv {
        match &launch.node {
            Some(node) => self.node_env(node),
            None => NodeEnv::new(),
        }
    }

    /// A text the core places at the very top of the prompt of an agent
    /// session being launched (API 0.13): before `# PDO Runtime Preamble` for
    /// a Node, before the runtime preamble of the Pipeline Manager and of the
    /// library assistant. Never asked for a script Node. By default,
    /// [`Self::node_preamble`] for a Node and nothing for the other sessions.
    ///
    /// Two extensions' texts follow each other in registration order. Called
    /// on the daemon's spawn path: keep it quick. A panic adds nothing.
    fn session_preamble(&self, launch: &SessionLaunch) -> Option<String> {
        launch
            .node
            .as_ref()
            .and_then(|node| self.node_preamble(node))
    }

    /// Why `trigger` is suspended, if it is: a suspended Trigger fires neither
    /// on its schedule nor by "Run now", and the interface shows it switched
    /// off with this reason. Asked again at every read and every due tick, so
    /// the suspension lifts as soon as it answers `None`. Not suspended by
    /// default.
    ///
    /// Between two extensions, the first that suspends wins. Called on the
    /// daemon's request path: keep it quick. A panic suspends nothing.
    fn trigger_suspension(&self, _trigger: &TriggerRef) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Verb;

    /// An extension that overrides nothing but its manifest.
    struct Bare;

    impl Extension for Bare {
        fn manifest(&self) -> Manifest {
            Manifest::new("bare", "Bare", "0.0.0")
        }
    }

    #[test]
    fn every_hook_defaults_to_the_neutral_answer() {
        let ext = Bare;
        let (parts, ()) = axum::http::Request::builder()
            .uri("/runs")
            .body(())
            .unwrap()
            .into_parts();
        let actor = Actor::LocalOperator;
        let op = Operation::new(ResourceKind::Project, Verb::Update).on("p1");

        assert_eq!(ext.identify(&parts), None);
        assert_eq!(ext.admit(&parts), Admission::Admit);
        assert_eq!(ext.display_name(&actor), None);
        assert_eq!(ext.authorize(&actor, &op), Authorization::Allow);
        assert!(ext
            .on_create(&actor, &EntityRef::new(ResourceKind::Project, "p1"))
            .is_empty());
        assert_eq!(
            ext.validate_create(&actor, &ResourceKind::Run, &serde_json::json!({ "k": "v" })),
            Ok(())
        );
        assert!(ext
            .on_create_with_data(
                &actor,
                &EntityRef::new(ResourceKind::Run, "r1"),
                Some(&serde_json::json!(1))
            )
            .is_empty());
        ext.start(&ExtensionContext::new("/tmp/bare"));
        assert!(ExtensionContext::new("/tmp/bare")
            .host
            .authorize(&actor, &op)
            .is_allowed());
        ext.on_event(&serde_json::json!({ "kind": "RunCreated" }));
        assert!(ext.routes().is_none());
        assert!(ext.ui().is_none());
        assert!(ext.cli().is_none());
        assert_eq!(
            ext.run_cli(&CliInvocation::new(vec![], "", actor.clone()))
                .code,
            1
        );
        assert!(ext.skills().is_empty());
        let launch = NodeLaunch::new("r1", "n1", 1, crate::NodeKind::Agent, "/repo");
        assert!(ext.node_env(&launch).is_empty());
        assert_eq!(ext.node_preamble(&launch), None);
        let manager = SessionLaunch::new(crate::SessionRef::new(
            "pdo-mgr-r1",
            crate::SessionKind::Manager,
        ));
        assert!(ext.session_env(&manager).is_empty());
        assert_eq!(ext.session_preamble(&manager), None);
        assert_eq!(
            ext.trigger_suspension(&TriggerRef::new("t1", "Nightly", None)),
            None
        );
        assert_eq!(ext.manifest().id, "bare");
    }

    #[test]
    fn extensions_are_object_safe() {
        let extensions: Vec<Box<dyn Extension>> = vec![Box::new(Bare)];
        assert_eq!(extensions.len(), 1);
    }

    #[test]
    fn a_context_carries_the_neutral_host_until_given_one() {
        let context = ExtensionContext::new("/tmp/x");
        assert_eq!(context, context.clone());
        let other = ExtensionContext::new("/tmp/x").with_host(Arc::new(NeutralHost));
        assert_ne!(context, other, "another host");
        assert!(format!("{context:?}").contains("/tmp/x"));
    }
}
