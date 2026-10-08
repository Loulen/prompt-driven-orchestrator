//! The event bus: where the core publishes what happened, as the JSON the daemon
//! broadcasts on `/ws` (D20). The front and the extensions read the same thing.

use std::borrow::Cow;

use serde_json::Value;
use tokio::sync::broadcast;

use crate::Operation;

/// The field of a message of the bus naming the read that guards it (an
/// [`Operation`] with the verb `read`): `/ws` relays the message only to the
/// connections whose Actor may run that read, then without the field. A
/// message without it reaches every connection, as before.
pub const READ_GUARD: &str = "read_guard";

/// `message` (an object) guarded by `read`.
pub fn guarded(mut message: Value, read: &Operation) -> Value {
    if let (Some(fields), Ok(read)) = (message.as_object_mut(), serde_json::to_value(read)) {
        fields.insert(READ_GUARD.to_string(), read);
    }
    message
}

/// `message` without its guard: what leaves the daemon of a guarded message.
pub fn unguarded(message: &Value) -> Cow<'_, Value> {
    if message.get(READ_GUARD).is_none() {
        return Cow::Borrowed(message);
    }
    let mut stripped = message.clone();
    if let Some(fields) = stripped.as_object_mut() {
        fields.remove(READ_GUARD);
    }
    Cow::Owned(stripped)
}

/// Publish events and subscribe to them.
pub trait EventBus: Send + Sync {
    /// Publish one event. Never blocks and never fails: an event nobody listens
    /// to is dropped.
    fn publish(&self, event: Value);

    /// A new subscription, receiving every event published from now on.
    fn subscribe(&self) -> broadcast::Receiver<Value>;
}

/// The in-process bus, over a tokio broadcast channel. A subscriber that lags
/// more than `capacity` events behind loses the oldest ones.
pub struct BroadcastEventBus {
    tx: broadcast::Sender<Value>,
}

impl BroadcastEventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// The bus over an existing channel: what the producers that still send on
    /// the raw channel send is published on the bus.
    pub fn from_sender(tx: broadcast::Sender<Value>) -> Self {
        Self { tx }
    }
}

impl EventBus for BroadcastEventBus {
    fn publish(&self, event: Value) {
        // `Err` only means "no subscriber right now".
        let _ = self.tx.send(event);
    }

    fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guarded_message_names_its_read_beside_its_fields() {
        let read = Operation::new(crate::ResourceKind::Trigger, crate::Verb::Read)
            .on("t1")
            .in_projects(["p1"]);
        assert_eq!(
            guarded(
                serde_json::json!({ "type": "trigger_updated", "trigger_id": "t1" }),
                &read
            ),
            serde_json::json!({
                "type": "trigger_updated",
                "trigger_id": "t1",
                "read_guard": {
                    "resource": "trigger", "verb": "read", "target": "t1", "projects": ["p1"],
                },
            })
        );
    }

    #[tokio::test]
    async fn a_subscriber_receives_what_is_published_after_it_subscribed() {
        let bus = BroadcastEventBus::new(8);
        bus.publish(serde_json::json!({ "kind": "lost" }));
        let mut rx = bus.subscribe();
        bus.publish(serde_json::json!({ "kind": "RunCreated" }));
        assert_eq!(rx.recv().await.unwrap()["kind"], "RunCreated");
    }

    #[tokio::test]
    async fn what_is_sent_on_the_channel_reaches_the_bus_subscribers() {
        let (tx, _) = broadcast::channel(8);
        let bus = BroadcastEventBus::from_sender(tx.clone());
        let mut rx = bus.subscribe();
        tx.send(serde_json::json!({ "type": "pipeline_changed" }))
            .unwrap();
        bus.publish(serde_json::json!({ "type": "event" }));
        assert_eq!(rx.recv().await.unwrap()["type"], "pipeline_changed");
        assert_eq!(rx.recv().await.unwrap()["type"], "event");
    }
}
