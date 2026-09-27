//! The event bus: where the core publishes what happened, as the JSON the daemon
//! broadcasts on `/ws` (D20). The front and the extensions read the same thing.

use serde_json::Value;
use tokio::sync::broadcast;

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
