use serde::Serialize;

use crate::{
    Actor, ActorDisplay, Attributes, Authorization, EntityRef, HostedConfig, HostedGesture,
    HostedRefusal, HostedSession, LiveArtifact, NewRun, OpenSession, OperationApplied, Presence,
    Resource, RunCostRead, RunCreation, RunNotCreated,
};

/// The `type` of an event an extension publishes, on `/ws` and in the other
/// extensions' `on_event`.
pub const EXTENSION_EVENT: &str = "extension_event";

/// What the core offers an extension for its **own** mutations (ADR-0081,
/// amendment #34), so that two extensions installed together interoperate
/// without naming each other. Handed over in
/// [`ExtensionContext::host`](crate::ExtensionContext::host) when the daemon
/// starts.
///
/// A mutation of the extension, on an entity it owns
/// ([`ResourceKind::Extension`](crate::ResourceKind::Extension)), goes like a
/// mutation of the core:
///
/// 1. [`Host::authorize`] the Operation; on a refusal, change nothing and
///    answer the [`Denied`](crate::Denied) of
///    [`Authorization::into_result`];
/// 2. run it; for a creation, store and return the attributes
///    [`Host::on_create`] gives;
/// 3. once it succeeded, [`Host::publish`] what happened, if anything, then
///    [`Host::applied`].
///
/// The actor of a request is the one the core identified: a route of the
/// extension reads it with axum's `Extension<Actor>` extractor.
///
/// `publish` and `applied` never wait for an extension (each one receives its
/// events on its own thread). `authorize` and `on_create` call every other
/// extension's hook on the caller's thread, as the core does for its own
/// mutations: they are as quick as those hooks, which must be quick. Call
/// them from a route, a thread or a task alike, never from the extension's
/// own `authorize` or `on_create`: the Host calls those back, without end.
pub trait Host: Send + Sync {
    /// Whether `actor` may run `operation`: the Autorisation d'opération of
    /// **every** extension, the calling one included, the first refusal
    /// winning with its reason. An extension that panics refuses.
    fn authorize(&self, actor: &Actor, operation: &crate::Operation) -> Authorization;

    /// Broadcast `event` on `/ws` and to the other extensions' `on_event`,
    /// wrapped in the envelope of [`ExtensionEvent::to_event`]. Never waits
    /// for an extension. An event [guarded](ExtensionEvent::guarded_by) by a
    /// read reaches only the `/ws` connections whose Actor may run that read
    /// (API 0.11), and never carries its guard.
    fn publish(&self, event: ExtensionEvent);

    /// Tell the core that an authorized mutation succeeded: `applied` is
    /// broadcast as the other mutations' `OperationApplied` (on `/ws` and to
    /// every extension's `on_event`). Never waits for an extension.
    fn applied(&self, applied: OperationApplied);

    /// The attributes the **other** extensions stamp on `entity`, which
    /// `actor` just created, each key prefixed with its extension's id
    /// (`<id>/<key>`). The extension stores them and returns them with the
    /// entity; the core keeps none of them. A panicking extension stamps
    /// nothing.
    fn on_create(&self, actor: &Actor, entity: &EntityRef) -> Attributes;

    /// How `actor` shows to a person: the first
    /// [`Extension::display_name`](crate::Extension::display_name) that knows
    /// it, the calling extension's included. `None` when none does (and by
    /// default): show the Actor as you would without. Never call it from your
    /// own `display_name`: the Host calls it back, without end.
    fn display_name(&self, _actor: &Actor) -> Option<ActorDisplay> {
        None
    }

    /// The catalogue of the Ressources: the core's, then those each
    /// registered extension declares in its [`Manifest`](crate::Manifest), in
    /// registration order. By default, the core's alone.
    fn catalogue(&self) -> Vec<Resource> {
        crate::core_resources()
    }

    /// Create a Run on behalf of `actor` (API 0.9), as `POST /runs` creates
    /// one for that caller: the same Autorisation d'opération (`run` ×
    /// `create`, in the Projet it names, ADR-0087), the same checks of
    /// the extension data ([`Extension::validate_create`](crate::Extension::validate_create),
    /// the calling extension's included), the same attributes stamped before
    /// its first Node. Answers the new Run's id, or the refusal `POST /runs`
    /// would have answered.
    ///
    /// The Run is a root Run (no node session), created by `actor`
    /// (`created_by`). Never call it from a hook of the creation it starts
    /// (`authorize`, `validate_create`, `on_create*`): those are asked again,
    /// without end. By default (and with the [`NeutralHost`]), `503`: no Run
    /// is created.
    fn create_run(&self, _actor: &Actor, _run: NewRun) -> RunCreation {
        Box::pin(std::future::ready(Err(RunNotCreated::unavailable(
            "this host cannot create a Run",
        ))))
    }

    /// The name of the Projet `project_id` (API 0.11), as a person reads it
    /// (`P1`), or `None` for an id the core does not know (and by default):
    /// show the id then. Synchronous and quick, so an extension may call it
    /// from its own `authorize` to name the Projet in a refusal's reason. It
    /// follows a renaming as soon as the Projet's `project_changed` is
    /// announced.
    fn project_name(&self, _project_id: &str) -> Option<String> {
        None
    }

    /// Tell the core that the suspensions of the Triggers changed (API 0.11):
    /// what [`Extension::trigger_suspension`](crate::Extension::trigger_suspension)
    /// answers is no longer what it answered, for a cause outside any
    /// mutation of a Trigger (a cause the extension holds itself). The
    /// core announces a `trigger_updated` without `trigger_id` on `/ws`, and
    /// the list of the Triggers reads them all again. Never waits. Nothing by
    /// default.
    fn triggers_changed(&self) {}

    /// The Actors who have the Run `run_id` open in a browser right now (API
    /// 0.12), by order of arrival: one [`Presence`] per Actor, whatever its
    /// count of tabs. Empty for a Run nobody has open, a Run the core does
    /// not know, and by default. Synchronous and quick. Each arrival and each
    /// departure is announced too ([`PRESENCE_ARRIVED`](crate::PRESENCE_ARRIVED),
    /// [`PRESENCE_LEFT`](crate::PRESENCE_LEFT)).
    fn presences(&self, _run_id: &str) -> Vec<Presence> {
        Vec::new()
    }

    /// The cost of the Run `run_id` (API 0.12), the one its Info tab shows:
    /// its amount, whether it is a lower bound, and why it is unknown.
    /// `None` for a Run the core does not know, and by default. Read on
    /// demand from the Run's sessions: await it from a route or a task, not
    /// from a hook.
    fn run_cost(&self, _run_id: &str) -> RunCostRead {
        Box::pin(std::future::ready(None))
    }

    /// Have the core open the Session hébergée `open` on behalf of `actor`
    /// (API 0.19, ADR-0092 of the core): an agent or a shell, outside any
    /// Run, in the folder it names, which the core never writes into. The
    /// Operation `session` × `open` (in its Projet) is asked first, then the
    /// Cap de sessions concurrentes: a full Cap refuses at once (`409`),
    /// nothing waits. The Contribution au lancement of every extension is
    /// asked with `actor` as who starts it, for an agent and a shell alike;
    /// the agent reads it, then the initial prompt, and nothing of the core.
    /// Its environment holds `PDO_SESSION_ID` and `PDO_DAEMON_URL`.
    ///
    /// Answers the session, `running`; `409` for a key the extension already
    /// gave, `400` for a folder that does not resolve or a harness the core
    /// does not know, `403` for a refused Operation. Each change of its state
    /// is published ([`HostedSessionChanged`](crate::HostedSessionChanged)).
    /// By default (and with the [`NeutralHost`]), `503`: nothing is opened.
    fn open_session(&self, _actor: &Actor, _open: OpenSession) -> HostedGesture {
        no_hosted_session()
    }

    /// The Session hébergée the calling extension opened under `key` (API
    /// 0.19), ended ones included; `None` for a key it never gave, and by
    /// default. Synchronous and quick.
    fn hosted_session(&self, _key: &str) -> Option<HostedSession> {
        None
    }

    /// Every Session hébergée the calling extension opened (API 0.19), in
    /// the order they were opened, ended ones included. Empty by default.
    fn hosted_sessions(&self) -> Vec<HostedSession> {
        Vec::new()
    }

    /// Terminate the Session hébergée `key` of the calling extension on
    /// behalf of `actor` (API 0.19): the Operation `session` × `terminate`
    /// (on the session, in its Projet) is asked first, then its process is
    /// killed and it ends (`terminated`). Its record stays. An ended session
    /// is answered as it is; `404` for a key it never gave. By default,
    /// `503`.
    fn terminate_session(&self, _actor: &Actor, _key: &str) -> HostedGesture {
        no_hosted_session()
    }

    /// Relaunch the ended Session hébergée `key` of the calling extension on
    /// behalf of `actor` (API 0.19), who becomes who started it: the
    /// Operation `session` × `relaunch` first, then the Cap (a full Cap
    /// refuses at once, `409`), then the Contribution au lancement asked
    /// anew. An agent resumes its conversation when its harness re-enters one
    /// by the identity pinned at its launch and its harness did not change;
    /// otherwise it starts a new one, reading the extensions' text and its
    /// initial prompt. A shell starts anew. It runs on its configuration as
    /// it now stands ([`Host::configure_session`]); its Artefacts live stay.
    ///
    /// Answers the session, `running`; `409` for a session that is not
    /// `ended`, `404` for a key it never gave. By default, `503`.
    fn relaunch_session(&self, _actor: &Actor, _key: &str) -> HostedGesture {
        no_hosted_session()
    }

    /// Change the configuration (harness, model, effort, initial prompt) of
    /// the agent session `key` of the calling extension on behalf of `actor`
    /// (API 0.19): the Operation `session` × `configure` first. `config`
    /// replaces the whole configuration. A running agent keeps the one it was
    /// launched with: the new one applies at its next relaunch, its initial
    /// prompt only when that relaunch starts a new conversation. Answers the session with its new configuration; `400`
    /// for a shell or a harness the core does not know, `404` for a key it
    /// never gave. By default, `503`.
    fn configure_session(
        &self,
        _actor: &Actor,
        _key: &str,
        _config: HostedConfig,
    ) -> HostedGesture {
        no_hosted_session()
    }

    /// Set the Zoom du terminal of the Session hébergée `key` of the calling
    /// extension on behalf of `actor` (API 0.19): the Operation `session` ×
    /// `zoom` first. `zoom` is in percent, from
    /// [`MIN_TERMINAL_ZOOM`](crate::MIN_TERMINAL_ZOOM) to
    /// [`MAX_TERMINAL_ZOOM`](crate::MAX_TERMINAL_ZOOM); it is recorded,
    /// published at once and applies to its terminal on every Poste, live or
    /// ended, agent or shell. Answers the session with its new zoom; `400`
    /// for a zoom out of that range, `404` for a key it never gave. By
    /// default, `503`.
    fn zoom_session(&self, _actor: &Actor, _key: &str, _zoom: u16) -> HostedGesture {
        no_hosted_session()
    }

    /// Lay `text` in the input of the live Session hébergée `key` of the
    /// calling extension on behalf of `actor` (API 0.19), as a paste,
    /// **without** submitting it: it waits for a person to complete it and
    /// press Enter. The Operation `session` × `type` first. Answers the
    /// session; `400` for an empty text, `409` for a session that is not
    /// running, `404` for a key it never gave. By default, `503`.
    fn type_in_session(&self, _actor: &Actor, _key: &str, _text: &str) -> HostedGesture {
        no_hosted_session()
    }

    /// Forget the ended Session hébergée `key` of the calling extension on
    /// behalf of `actor` (API 0.19): the Operation `session` × `forget`
    /// first, then its record and its state folder (its Artefacts live
    /// included) are erased, and its key is free again. Answers the session
    /// as it was last recorded; `409` for a session that is not `ended`,
    /// `404` for a key it never gave. By default, `503`.
    fn forget_session(&self, _actor: &Actor, _key: &str) -> HostedGesture {
        no_hosted_session()
    }

    /// The Artefacts live of the Session hébergée `key` of the calling
    /// extension (API 0.19): the files its agent wrote in its
    /// `PDO_LIVE_DIR`, by path. `404` for a key it never gave. By default,
    /// `503`. Synchronous and quick.
    fn live_artifacts(&self, _key: &str) -> Result<Vec<LiveArtifact>, HostedRefusal> {
        Err(HostedRefusal::unavailable(NO_HOSTED_SESSION))
    }

    /// The content of the Artefact live `path` (as
    /// [`Host::live_artifacts`] names it) of the Session hébergée `key` of
    /// the calling extension (API 0.19). `404` for a key it never gave or a
    /// file that is not there, `400` for a path that leaves `PDO_LIVE_DIR`.
    /// By default, `503`. Synchronous.
    fn live_artifact(&self, _key: &str, _path: &str) -> Result<Vec<u8>, HostedRefusal> {
        Err(HostedRefusal::unavailable(NO_HOSTED_SESSION))
    }
}

const NO_HOSTED_SESSION: &str = "this host holds no hosted session";

/// The answer of a gesture on a Session hébergée of a host that holds none.
fn no_hosted_session() -> HostedGesture {
    Box::pin(std::future::ready(Err(HostedRefusal::unavailable(
        NO_HOSTED_SESSION,
    ))))
}

/// The host of an extension alone: everything allowed, events go nowhere, no
/// attribute. The one an [`ExtensionContext`](crate::ExtensionContext) holds
/// until the core gives its own, and a stand-in for an extension's tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NeutralHost;

impl Host for NeutralHost {
    fn authorize(&self, _actor: &Actor, _operation: &crate::Operation) -> Authorization {
        Authorization::Allow
    }

    fn publish(&self, _event: ExtensionEvent) {}

    fn applied(&self, _applied: OperationApplied) {}

    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }
}

/// An event an extension publishes about its own entities: a name it
/// chooses (`note_changed`) and a payload it owns, optionally guarded by the
/// read that protects it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExtensionEvent {
    pub name: String,
    pub payload: serde_json::Value,
    /// The read (an [`Operation`](crate::Operation) with the verb `read`:
    /// Ressource, target, Projets) a `/ws` connection's Actor must be allowed
    /// to run to receive the event (API 0.11). `None` (the default): every
    /// connection receives it. Never part of the envelope.
    #[serde(skip)]
    pub guard: Option<crate::Operation>,
}

impl ExtensionEvent {
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            payload,
            guard: None,
        }
    }

    /// The event, guarded by `read` (API 0.11): the core asks each `/ws`
    /// connection's Actor for that read before sending it (the decision kept
    /// a few seconds), and sends it without the guard. The read of the entity
    /// the event is about: `Operation::new(kind, Verb::Read).on(id)
    /// .in_projects(projects)`. A guard that is no read refuses every
    /// connection. Without an extension that restricts the reads, every
    /// connection receives it. The other extensions' `on_event` receive it
    /// as before, whatever the guard.
    pub fn guarded_by(mut self, read: crate::Operation) -> Self {
        self.guard = Some(read);
        self
    }

    /// The event as `/ws` and `on_event` carry it, published by the extension
    /// `extension`: `{"type": "extension_event", "extension": <id>, "name":
    /// <name>, "payload": <payload>}`. The envelope is the core's, frozen like
    /// the rest of `/ws`; the payload is the extension's.
    pub fn to_event(&self, extension: &str) -> serde_json::Value {
        serde_json::json!({
            "type": EXTENSION_EVENT,
            "extension": extension,
            "name": self.name,
            "payload": self.payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Operation, ResourceKind, Verb};

    #[test]
    fn an_event_is_wrapped_in_the_envelope_of_ws() {
        let event = ExtensionEvent::new("note_changed", serde_json::json!({ "id": "note-12" }));
        assert_eq!(
            event.to_event("test"),
            serde_json::json!({
                "type": "extension_event",
                "extension": "test",
                "name": "note_changed",
                "payload": { "id": "note-12" },
            })
        );
    }

    #[test]
    fn a_guard_never_enters_the_envelope() {
        let read = Operation::new(ResourceKind::extension("test", "note"), Verb::Read)
            .on("note-12")
            .in_projects(["p2"]);
        let event = ExtensionEvent::new("note_changed", serde_json::json!({ "id": "note-12" }))
            .guarded_by(read.clone());
        assert_eq!(event.guard, Some(read));
        assert_eq!(
            event.to_event("test"),
            ExtensionEvent::new("note_changed", serde_json::json!({ "id": "note-12" }))
                .to_event("test")
        );
        assert_eq!(
            ExtensionEvent::new("x", serde_json::Value::Null).guard,
            None,
            "unguarded by default"
        );
    }

    #[test]
    fn the_neutral_host_allows_everything_and_stamps_nothing() {
        let host: &dyn Host = &NeutralHost;
        let kind = ResourceKind::extension("test", "note");
        let op = Operation::new(kind.clone(), Verb::Create);
        assert!(host.authorize(&Actor::LocalOperator, &op).is_allowed());
        assert!(host
            .on_create(
                &Actor::LocalOperator,
                &EntityRef::new(kind.clone(), "note-1")
            )
            .is_empty());
        assert_eq!(host.display_name(&Actor::LocalOperator), None);
        host.publish(ExtensionEvent::new("x", serde_json::Value::Null));
        host.applied(OperationApplied::new(Actor::LocalOperator, op, None));
        assert_eq!(host.catalogue(), crate::core_resources());
        assert_eq!(host.project_name("prj-1"), None);
        host.triggers_changed();
    }

    /// API 0.11 is additive: a host written against 0.10 compiles unchanged
    /// and answers no name.
    #[test]
    fn a_host_without_the_new_reads_names_no_projet() {
        struct Older;
        impl Host for Older {
            fn authorize(&self, _actor: &Actor, _operation: &crate::Operation) -> Authorization {
                Authorization::Allow
            }
            fn publish(&self, _event: ExtensionEvent) {}
            fn applied(&self, _applied: OperationApplied) {}
            fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
                Attributes::new()
            }
        }
        assert_eq!(Older.project_name("prj-1"), None);
        Older.triggers_changed();
        assert!(Older.presences("r1").is_empty());
    }

    #[tokio::test]
    async fn the_neutral_host_sees_nobody_and_no_cost() {
        assert!(NeutralHost.presences("r1").is_empty());
        assert_eq!(NeutralHost.run_cost("r1").await, None);
    }

    #[tokio::test]
    async fn the_neutral_host_holds_no_hosted_session() {
        let refused = NeutralHost
            .open_session(&Actor::LocalOperator, crate::OpenSession::shell("s"))
            .await
            .unwrap_err();
        assert_eq!(refused.status, 503);
        assert_eq!(NeutralHost.hosted_session("s"), None);
        assert!(NeutralHost.hosted_sessions().is_empty());
        let refused = NeutralHost
            .terminate_session(&Actor::LocalOperator, "s")
            .await
            .unwrap_err();
        assert_eq!(refused.status, 503);
        for gesture in [
            NeutralHost.relaunch_session(&Actor::LocalOperator, "s"),
            NeutralHost.configure_session(
                &Actor::LocalOperator,
                "s",
                crate::HostedConfig::default(),
            ),
            NeutralHost.type_in_session(&Actor::LocalOperator, "s", "hi"),
            NeutralHost.forget_session(&Actor::LocalOperator, "s"),
        ] {
            assert_eq!(gesture.await.unwrap_err().status, 503);
        }
        assert_eq!(NeutralHost.live_artifacts("s").unwrap_err().status, 503);
        assert_eq!(
            NeutralHost.live_artifact("s", "a.md").unwrap_err().status,
            503
        );
    }

    #[tokio::test]
    async fn the_neutral_host_creates_no_run() {
        let refused = NeutralHost
            .create_run(&Actor::LocalOperator, NewRun::new("relay", "go"))
            .await
            .unwrap_err();
        assert_eq!(refused.status, 503);
        assert_eq!(refused.message, "this host cannot create a Run");
    }
}
