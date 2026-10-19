//! The instruments of the instance's metrics, built over any meter: the layer
//! that measures knows nothing of where the measures go (OTLP today, other
//! readers beside it tomorrow). The live counts (sessions, WebSockets) are
//! observed from counters the hub keeps, so a rebuilt provider reads the same
//! count instead of restarting from zero.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use opentelemetry::metrics::{Counter, Histogram, Meter, ObservableGauge};
use opentelemetry::KeyValue;

use crate::domain::attributes::{keys, metrics};

/// The counts the observable gauges read.
#[derive(Debug, Default)]
pub struct LiveCounts {
    /// `-1` until a first count lands: the gauge stays silent until then.
    pub sessions: AtomicI64,
}

pub struct Instruments {
    pub runs_started: Counter<u64>,
    pub runs_finished: Counter<u64>,
    pub runs_failed: Counter<u64>,
    pub node_duration: Histogram<f64>,
    pub node_active_duration: Histogram<f64>,
    pub node_wait_duration: Histogram<f64>,
    pub waits_declared: Counter<u64>,
    pub cost: Counter<f64>,
    pub tokens: Counter<u64>,
    pub http_duration: Histogram<f64>,
    _sessions: ObservableGauge<i64>,
    _websockets: ObservableGauge<i64>,
}

impl Instruments {
    pub fn new(meter: &Meter, live: Arc<LiveCounts>) -> Self {
        let counter = |name: &'static str, description: &'static str| {
            meter
                .u64_counter(name)
                .with_description(description)
                .build()
        };
        let seconds = |name: &'static str, description: &'static str| {
            meter
                .f64_histogram(name)
                .with_description(description)
                .with_unit("s")
                .build()
        };
        Self {
            runs_started: counter(metrics::RUNS_STARTED, "Runs launched"),
            runs_finished: counter(metrics::RUNS_FINISHED, "Runs ended, by outcome"),
            runs_failed: counter(metrics::RUNS_FAILED, "Runs failed"),
            node_duration: seconds(metrics::NODE_DURATION, "Node wall-clock"),
            node_active_duration: seconds(
                metrics::NODE_ACTIVE_DURATION,
                "Node wall-clock minus its declared wait",
            ),
            node_wait_duration: seconds(
                metrics::NODE_WAIT_DURATION,
                "Node time spent in a declared wait",
            ),
            waits_declared: counter(metrics::WAITS_DECLARED, "Declared waits opened"),
            cost: meter
                .f64_counter(metrics::COST)
                .with_description("Cost of the ended Runs")
                .with_unit("USD")
                .build(),
            tokens: counter(metrics::TOKENS, "Tokens of the ended Runs, by type"),
            http_duration: seconds(metrics::HTTP_DURATION, "HTTP request latency"),
            _sessions: meter
                .i64_observable_gauge(metrics::SESSIONS_LIVE)
                .with_description("Agent sessions alive")
                .with_callback(move |observer| {
                    let live = live.sessions.load(Ordering::Relaxed);
                    if live >= 0 {
                        observer.observe(live, &[]);
                    }
                })
                .build(),
            _websockets: meter
                .i64_observable_gauge(metrics::WEBSOCKETS_OPEN)
                .with_description("WebSocket connections open, by channel")
                .with_callback(|observer| {
                    for (channel, open) in pdo_kernel::live_connections::snapshot() {
                        observer.observe(open, &[KeyValue::new(keys::CONNECTION, channel)]);
                    }
                })
                .build(),
        }
    }
}

/// Attributes as the SDK takes them.
pub fn key_values(attributes: &[(&'static str, String)]) -> Vec<KeyValue> {
    attributes
        .iter()
        .map(|(key, value)| KeyValue::new(*key, value.clone()))
        .collect()
}
