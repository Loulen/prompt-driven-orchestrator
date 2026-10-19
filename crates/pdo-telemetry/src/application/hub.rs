//! The hub: the providers the instance's measures go through, rebuilt when the
//! collector changes — no restart. Each channel (the OTLP export, `/metrics`)
//! plugs into the same providers, so both carry the same measures. With no
//! channel on, there are none: every measure is a no-op and nothing connects
//! anywhere (ADR-0090 §2). The `/metrics` reader is plugged in whenever there
//! are providers, so its switch only opens or closes the scrape: it never
//! restarts the OTLP export. Whether the Actor goes with the measures is read
//! at each measure, so switching it rebuilds nothing either.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::{ExtensionHost, NoExtensions};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use serde::Serialize;
use tokio::sync::Mutex;
use ts_rs::TS;

use super::instruments::{Instruments, LiveCounts};
use super::ports::{
    Builders, Exporters, LiveSessions, MetricsEndpoint, Providers, Scrape, TelemetrySettingSource,
};
use super::status::{ExportStatus, LastExport};
use crate::domain::attributes::{catalogue, keys, TelemetryCatalogue};
use crate::domain::setting::ExportTarget;

/// The instance the measures come from.
#[derive(Debug, Clone)]
pub struct Identity {
    /// `service.instance.id`: the host and the daemon's port.
    pub instance: String,
    pub version: String,
}

/// The providers of one set of channels, with the instruments built over them.
pub struct Active {
    providers: Providers,
    pub instruments: Instruments,
    pub tracer: SdkTracer,
    /// Whether the OTLP export is one of the channels.
    otlp: bool,
    /// What `/metrics` renders, when its reader could be plugged in.
    scrape: Option<Arc<dyn Scrape>>,
}

/// What the providers are built for: they are rebuilt only when it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Built {
    otlp: Option<ExportTarget>,
}

/// The telemetry of the instance.
pub struct Telemetry {
    setting: Arc<dyn TelemetrySettingSource>,
    exporters: Arc<dyn Exporters>,
    /// `None`: `/metrics` cannot be served, whatever the setting says.
    endpoint: Option<Arc<dyn MetricsEndpoint>>,
    sessions: Option<Arc<dyn LiveSessions>>,
    identity: Identity,
    interval: Duration,
    active: RwLock<Option<Arc<Active>>>,
    /// What the providers were built for, `None` while there are none; held
    /// across a reload so two reloads never race.
    applied: Mutex<Option<Built>>,
    /// The `/metrics` switch, read at each scrape.
    prometheus: AtomicBool,
    include_actor: AtomicBool,
    status: Arc<ExportStatus>,
    live: Arc<LiveCounts>,
    /// Asked before a test export, which sends data out of the instance.
    extensions: Arc<dyn ExtensionHost>,
}

/// `GET /telemetry/status`: the export as it runs now.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TelemetryStatus {
    /// Whether the export runs: the switch is on and a collector is named.
    pub exporting: bool,
    /// Whether `/metrics` answers.
    pub metrics_endpoint: bool,
    /// Whether who launched a Run, and who answered it, goes with its measures.
    pub include_actor: bool,
    /// Seconds between two metric exports.
    pub interval_secs: u64,
    pub last_export: Option<LastExport>,
    pub sent: TelemetryCatalogue,
}

/// Why a test export did not run.
#[derive(Debug)]
pub enum TestExportError {
    Refused(Denied),
    NotExporting,
}

impl Telemetry {
    pub fn new(
        setting: Arc<dyn TelemetrySettingSource>,
        exporters: Arc<dyn Exporters>,
        identity: Identity,
        interval: Duration,
    ) -> Self {
        Self {
            setting,
            exporters,
            endpoint: None,
            sessions: None,
            identity,
            interval,
            active: RwLock::new(None),
            applied: Mutex::new(None),
            prometheus: AtomicBool::new(false),
            include_actor: AtomicBool::new(false),
            status: Arc::new(ExportStatus::default()),
            live: Arc::new(LiveCounts {
                sessions: (-1).into(),
            }),
            extensions: Arc::new(NoExtensions),
        }
    }

    /// Ask `extensions` before a test export.
    pub fn with_extensions(mut self, extensions: Arc<dyn ExtensionHost>) -> Self {
        self.extensions = extensions;
        self
    }

    /// Serve `/metrics` through `endpoint` when its switch is on.
    pub fn with_metrics_endpoint(mut self, endpoint: Arc<dyn MetricsEndpoint>) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    /// Count the live sessions through `sessions`.
    pub fn with_sessions(mut self, sessions: Arc<dyn LiveSessions>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// The providers of the channels applied now, `None` when none is on.
    pub fn active(&self) -> Option<Arc<Active>> {
        self.active
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether the measures may name who launched a Run or answered it.
    pub fn include_actor(&self) -> bool {
        self.include_actor.load(Ordering::Relaxed)
    }

    /// Apply the setting as stored now: build the providers of a new
    /// collector, shut the old ones down. A no-op for the providers when the
    /// collector did not change and some channel was and stays on.
    pub async fn reload(&self) {
        // The setting is read under the lock: a reload that read an older
        // setting can never apply it after a newer one.
        let mut applied = self.applied.lock().await;
        let setting = self.setting.setting().await;
        self.include_actor
            .store(setting.include_actor, Ordering::Relaxed);
        let channels = setting.channels();
        if self.prometheus.swap(channels.prometheus, Ordering::Relaxed) != channels.prometheus {
            match channels.prometheus {
                true => tracing::info!("telemetry: /metrics on"),
                false => tracing::info!("telemetry: /metrics off"),
            }
        }
        let wanted = channels.any().then(|| Built {
            otlp: channels.otlp.clone(),
        });
        if *applied == wanted {
            return;
        }
        let next = match &wanted {
            Some(built) => match self.build(built) {
                Ok(active) => Some(Arc::new(active)),
                Err(error) => {
                    tracing::warn!(%error, "telemetry: cannot build the export");
                    None
                }
            },
            None => None,
        };
        let was = applied.as_ref().and_then(|b| b.otlp.clone());
        if was != channels.otlp {
            self.status.reset();
        }
        let failed = wanted.is_some() && next.is_none();
        let previous = std::mem::replace(
            &mut *self.active.write().unwrap_or_else(|e| e.into_inner()),
            next,
        );
        match (&was, &channels.otlp) {
            (Some(old), Some(t)) if old == t => {}
            (_, Some(t)) => tracing::info!(endpoint = %t.endpoint, "telemetry: OTLP export on"),
            (Some(_), None) => tracing::info!("telemetry: OTLP export off"),
            (None, None) => {}
        }
        // A build that failed is tried again by the next reload.
        *applied = if failed { None } else { wanted };
        drop(applied);
        if let Some(previous) = previous {
            // The shutdown flushes what is pending: it blocks, off the runtime.
            let _ = tokio::task::spawn_blocking(move || shutdown(&previous)).await;
        }
    }

    fn build(&self, built: &Built) -> Result<Active, String> {
        let resource = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "pdo"),
                KeyValue::new("service.version", self.identity.version.clone()),
                KeyValue::new(keys::INSTANCE, self.identity.instance.clone()),
            ])
            .build();
        let builders = Builders {
            meters: SdkMeterProvider::builder().with_resource(resource.clone()),
            tracers: SdkTracerProvider::builder().with_resource(resource),
        };
        let mut builders = match &built.otlp {
            Some(target) => self.attach_otlp(target, builders)?,
            None => builders,
        };
        let mut scrape = None;
        // Plugged in whatever its switch says: switching it then rebuilds
        // nothing.
        if let Some(endpoint) = &self.endpoint {
            let (meters, scraped) = endpoint.attach(builders.meters)?;
            builders.meters = meters;
            scrape = Some(scraped);
        }
        let providers = Providers {
            meters: builders.meters.build(),
            tracers: builders.tracers.build(),
        };
        let meter = providers.meters.meter("pdo");
        let tracer = providers.tracers.tracer("pdo");
        Ok(Active {
            instruments: Instruments::new(&meter, self.live.clone()),
            tracer,
            providers,
            otlp: built.otlp.is_some(),
            scrape,
        })
    }

    fn attach_otlp(&self, target: &ExportTarget, builders: Builders) -> Result<Builders, String> {
        self.exporters
            .attach(target, self.interval, self.status.clone(), builders)
    }

    /// The OTLP export's providers, `None` while it is off.
    fn exporting(&self) -> Option<Arc<Active>> {
        self.active().filter(|active| active.otlp)
    }

    /// What `/metrics` renders, `None` while it is off.
    fn scraped(&self) -> Option<Arc<dyn Scrape>> {
        if !self.prometheus.load(Ordering::Relaxed) {
            return None;
        }
        self.active()?.scrape.clone()
    }

    /// One scrape of `/metrics`: `None` while it is off, else the measures as
    /// they stand.
    pub fn scrape(&self) -> Option<Result<String, String>> {
        Some(self.scraped()?.render())
    }

    pub fn status(&self) -> TelemetryStatus {
        let active = self.active();
        TelemetryStatus {
            exporting: active.as_ref().is_some_and(|a| a.otlp),
            metrics_endpoint: self.scraped().is_some(),
            include_actor: self.include_actor(),
            interval_secs: self.interval.as_secs(),
            last_export: self.status.last(),
            sent: catalogue(),
        }
    }

    /// Export now, a test span included so the collector always receives
    /// something, and answer the status that followed.
    pub async fn test_export(&self, actor: &Actor) -> Result<TelemetryStatus, TestExportError> {
        let operation = Operation::new(
            ResourceKind::InstanceSettings,
            Verb::Action("test_telemetry_export".into()),
        );
        authorize(self.extensions.as_ref(), actor, &operation).map_err(TestExportError::Refused)?;
        let active = self.exporting().ok_or(TestExportError::NotExporting)?;
        active.tracer.start("pdo.telemetry.test").end();
        let flushed = active.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = flushed.providers.meters.force_flush();
            let _ = flushed.providers.tracers.force_flush();
        })
        .await;
        Ok(self.status())
    }

    /// Refresh the count of live sessions while the export runs, every
    /// `period`, until the daemon stops.
    pub async fn count_sessions(self: Arc<Self>, period: Duration) {
        let Some(sessions) = self.sessions.clone() else {
            return;
        };
        loop {
            if self.active().is_some() {
                if let Some(live) = sessions.live().await {
                    self.live
                        .sessions
                        .store(live, std::sync::atomic::Ordering::Relaxed);
                }
            }
            tokio::time::sleep(period).await;
        }
    }

    /// Flush and stop the export: the daemon is stopping.
    pub async fn shutdown(&self) {
        let previous = self
            .active
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(previous) = previous {
            let _ = tokio::task::spawn_blocking(move || shutdown(&previous)).await;
        }
    }
}

fn shutdown(active: &Active) {
    let _ = active.providers.meters.shutdown();
    let _ = active.providers.tracers.shutdown();
}
