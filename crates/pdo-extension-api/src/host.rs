use serde::Serialize;

use crate::{
    Actor, ActorDisplay, Attributes, Authorization, EntityRef, NewRun, OperationApplied, Resource,
    RunCreation, RunNotCreated,
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
    /// for an extension.
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
    /// `create`, in the Projet of its primary repository), the same checks of
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
/// chooses (`note_changed`) and a payload it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExtensionEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

impl ExtensionEvent {
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            payload,
        }
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
