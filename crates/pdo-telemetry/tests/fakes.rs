use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentelemetry::{KeyValue, Value};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader};
use opentelemetry_sdk::trace::InMemorySpanExporter;
use opentelemetry_sdk::Resource;
use pdo_run::domain::event::{Event, EventKind};
use pdo_telemetry::application::ports::{
    BoxFuture, Builders, Exporters, OtlpSettingSource, RunCosts, RunFactsSource,
};
use pdo_telemetry::application::status::ExportStatus;
use pdo_telemetry::domain::attributes::RunFacts;
use pdo_telemetry::domain::cost::{CostSlice, Tokens};
use pdo_telemetry::domain::setting::{ExportTarget, OtlpSetting};
use pdo_telemetry::{Identity, Telemetry};

/// The setting as a test sets it.
#[derive(Default)]
pub struct Setting(pub Mutex<OtlpSetting>);

impl Setting {
    pub fn set(&self, enabled: bool, endpoint: Option<&str>) {
        let mut setting = self.0.lock().unwrap();
        setting.enabled = enabled;
        setting.endpoint = endpoint.map(Into::into);
    }
}

impl OtlpSettingSource for Setting {
    fn otlp(&self) -> BoxFuture<'_, OtlpSetting> {
        let setting = self.0.lock().unwrap().clone();
        Box::pin(async move { setting })
    }
}

/// Exporters that keep what they receive, and count how often a target was
/// built: a build is the only way anything could connect anywhere.
#[derive(Default)]
pub struct InMemory {
    pub metrics: InMemoryMetricExporter,
    pub spans: InMemorySpanExporter,
    pub builds: AtomicUsize,
    pub targets: Mutex<Vec<ExportTarget>>,
}

impl Exporters for InMemory {
    fn attach(
        &self,
        target: &ExportTarget,
        _interval: Duration,
        _status: Arc<ExportStatus>,
        builders: Builders,
    ) -> Result<Builders, String> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        self.targets.lock().unwrap().push(target.clone());
        Ok(Builders {
            meters: builders.meters.with_reader(
                PeriodicReader::builder(self.metrics.clone())
                    .with_interval(Duration::from_secs(3600))
                    .build(),
            ),
            tracers: builders.tracers.with_simple_exporter(self.spans.clone()),
        })
    }
}

impl InMemory {
    /// Every data point exported so far: the metric's name and its attributes.
    pub fn points(&self) -> Vec<(String, Vec<KeyValue>)> {
        let mut points = Vec::new();
        for resource in self.metrics.get_finished_metrics().unwrap() {
            for scope in resource.scope_metrics() {
                for metric in scope.metrics() {
                    let name = metric.name().to_string();
                    for attributes in attributes_of(metric.data()) {
                        points.push((name.clone(), attributes));
                    }
                }
            }
        }
        points
    }

    pub fn resources(&self) -> Vec<Resource> {
        self.metrics
            .get_finished_metrics()
            .unwrap()
            .iter()
            .map(|r| r.resource().clone())
            .collect()
    }
}

fn attributes_of(data: &AggregatedMetrics) -> Vec<Vec<KeyValue>> {
    fn of<T: Copy>(data: &MetricData<T>) -> Vec<Vec<KeyValue>> {
        match data {
            MetricData::Sum(sum) => sum
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::Histogram(h) => h
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::Gauge(g) => g
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::ExponentialHistogram(_) => Vec::new(),
        }
    }
    match data {
        AggregatedMetrics::F64(d) => of(d),
        AggregatedMetrics::U64(d) => of(d),
        AggregatedMetrics::I64(d) => of(d),
    }
}

pub fn value_of<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a Value> {
    attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| &kv.value)
}

/// The facts of every Run: what the composition root resolves, a remote URL
/// for the repository.
pub struct Facts;

impl RunFactsSource for Facts {
    fn facts<'a>(&'a self, _run_id: &'a str) -> BoxFuture<'a, RunFacts> {
        Box::pin(async {
            RunFacts {
                project: Some("Core".into()),
                repository: Some("https://git.example.org/org/app.git".into()),
                branch: Some("main".into()),
                commit: Some("abc123".into()),
                pipeline: Some("implement".into()),
                harness: Some("run-harness".into()),
            }
        })
    }
}

/// One priced slice per Run.
pub struct Costs;

impl RunCosts for Costs {
    fn slices<'a>(&'a self, _run_id: &'a str) -> BoxFuture<'a, Vec<CostSlice>> {
        Box::pin(async {
            vec![CostSlice {
                harness: "node-harness".into(),
                model: Some("model-x".into()),
                usd: Some(1.5),
                tokens: Some(Tokens {
                    input: 10,
                    output: 20,
                    cache_read: 30,
                    cache_write: 40,
                }),
            }]
        })
    }
}

pub struct World {
    pub setting: Arc<Setting>,
    pub exporters: Arc<InMemory>,
    pub telemetry: Arc<Telemetry>,
}

pub fn world() -> World {
    let setting = Arc::new(Setting::default());
    let exporters = Arc::new(InMemory::default());
    let telemetry = Arc::new(Telemetry::new(
        setting.clone(),
        exporters.clone(),
        Identity {
            instance: "host:5272".into(),
            version: "0.0.0".into(),
        },
        Duration::from_secs(3600),
    ));
    World {
        setting,
        exporters,
        telemetry,
    }
}

pub fn event(
    second: u32,
    kind: EventKind,
    node: Option<&str>,
    payload: serde_json::Value,
) -> Event {
    Event {
        id: None,
        run_id: "run-1".into(),
        ts: format!("2033-03-10T10:{:02}:{:02}Z", second / 60, second % 60),
        kind,
        node_id: node.map(Into::into),
        iter: node.map(|_| 1),
        payload: Some(payload),
    }
}
