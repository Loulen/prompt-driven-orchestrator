use serde::Serialize;

use crate::{
    Actor, ActorDisplay, Attributes, Authorization, EntityRef, OperationApplied, Resource,
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
    }
}
