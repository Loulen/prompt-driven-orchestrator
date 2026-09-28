//! The delivery of the events to one extension: asynchronous and isolated.
//!
//! Each extension gets its own thread and its own bounded queue. Publishing
//! only pushes into the queues, never waits: an extension whose `on_event`
//! sleeps fills its own queue (the overflow is dropped and logged), one that
//! panics loses the event it panicked on. Neither delays the core nor another
//! extension.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;

use pdo_extension_api::Extension;
use serde_json::Value;

/// How many events an extension may lag behind before the next ones are
/// dropped.
pub(crate) const QUEUE_CAPACITY: usize = 1024;

/// The sending end of one extension's queue.
#[derive(Clone)]
pub(crate) struct Delivery {
    id: String,
    queue: SyncSender<Value>,
    /// Whether the overflow was already logged since the queue last accepted
    /// an event: one line per burst, not one per event.
    overflowing: Arc<AtomicBool>,
}

impl Delivery {
    /// Start the thread delivering to `extension`. It ends when every
    /// [`Delivery`] of the queue is dropped.
    pub(crate) fn start(id: String, extension: Arc<dyn Extension>) -> Self {
        let (queue, events) = sync_channel::<Value>(QUEUE_CAPACITY);
        let thread_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("pdo-ext-{id}"))
            .spawn(move || {
                for event in events {
                    if catch_unwind(AssertUnwindSafe(|| extension.on_event(&event))).is_err() {
                        tracing::warn!("extension `{thread_id}` panicked in on_event");
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::error!("failed to start the event delivery of extension `{id}`: {e}");
        }
        Self {
            id,
            queue,
            overflowing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Queue `event`, without waiting.
    pub(crate) fn send(&self, event: &Value) {
        match self.queue.try_send(event.clone()) {
            Ok(()) => self.overflowing.store(false, Ordering::Relaxed),
            Err(TrySendError::Full(_)) => {
                if !self.overflowing.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        "extension `{}` lags {QUEUE_CAPACITY} events behind: events dropped",
                        self.id
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::Manifest;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Recorder(Arc<Mutex<Vec<Value>>>, Duration);

    impl Extension for Recorder {
        fn manifest(&self) -> Manifest {
            Manifest::new("recorder", "Recorder", "0.0.0")
        }
        fn on_event(&self, event: &Value) {
            std::thread::sleep(self.1);
            if event["panic"] == true {
                panic!("asked to");
            }
            self.0.lock().unwrap().push(event.clone());
        }
    }

    fn wait_for(seen: &Arc<Mutex<Vec<Value>>>, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline, "only {:?}", seen.lock().unwrap());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn delivers_in_order_and_survives_a_panic() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let delivery = Delivery::start(
            "recorder".into(),
            Arc::new(Recorder(seen.clone(), Duration::ZERO)),
        );
        delivery.send(&serde_json::json!({ "n": 1 }));
        delivery.send(&serde_json::json!({ "panic": true }));
        delivery.send(&serde_json::json!({ "n": 2 }));
        wait_for(&seen, 2);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![serde_json::json!({ "n": 1 }), serde_json::json!({ "n": 2 })]
        );
    }

    #[test]
    fn a_slow_extension_never_makes_the_sender_wait() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let delivery = Delivery::start(
            "slow".into(),
            Arc::new(Recorder(seen, Duration::from_secs(60))),
        );
        let start = Instant::now();
        for n in 0..QUEUE_CAPACITY + 10 {
            delivery.send(&serde_json::json!({ "n": n }));
        }
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
