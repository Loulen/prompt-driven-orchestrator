//! The hub: the providers the instance's measures go through, rebuilt when the
//! setting changes — no restart. Off, there are none: every measure is a no-op
//! and nothing connects anywhere (ADR-0090 §2).

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
use super::ports::{Builders, Exporters, LiveSessions, OtlpSettingSource, Providers};
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

/// The providers of one target, with the instruments built over them.
pub struct Active {
    providers: Providers,
    pub instruments: Instruments,
    pub tracer: SdkTracer,
}

/// The telemetry of the instance.
pub struct Telemetry {
    setting: Arc<dyn OtlpSettingSource>,
    exporters: Arc<dyn Exporters>,
    sessions: Option<Arc<dyn LiveSessions>>,
    identity: Identity,
    interval: Duration,
    active: RwLock<Option<Arc<Active>>>,
    /// The target applied; held across a reload so two reloads never race.
    applied: Mutex<Option<ExportTarget>>,
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
        setting: Arc<dyn OtlpSettingSource>,
        exporters: Arc<dyn Exporters>,
        identity: Identity,
        interval: Duration,
    ) -> Self {
        Self {
            setting,
            exporters,
            sessions: None,
            identity,
            interval,
            active: RwLock::new(None),
            applied: Mutex::new(None),
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

    /// Count the live sessions through `sessions`.
    pub fn with_sessions(mut self, sessions: Arc<dyn LiveSessions>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// The providers of the target applied now, `None` when nothing exports.
    pub fn active(&self) -> Option<Arc<Active>> {
        self.active
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Apply the setting as stored now: build the providers of a new target,
    /// shut those of the old one down. A no-op when the target did not change.
    pub async fn reload(&self) {
        // The setting is read under the lock: a reload that read an older
        // setting can never apply it after a newer one.
        let mut applied = self.applied.lock().await;
        let target = self.setting.otlp().await.target();
        if *applied == target {
            return;
        }
        let next = match &target {
            Some(target) => match self.build(target) {
                Ok(active) => Some(Arc::new(active)),
                Err(error) => {
                    tracing::warn!(%error, "telemetry: cannot build the OTLP export");
                    None
                }
            },
            None => None,
        };
        self.status.reset();
        let previous = std::mem::replace(
            &mut *self.active.write().unwrap_or_else(|e| e.into_inner()),
            next,
        );
        match &target {
            Some(t) => tracing::info!(endpoint = %t.endpoint, "telemetry: OTLP export on"),
            None if previous.is_some() => tracing::info!("telemetry: OTLP export off"),
            None => {}
        }
        *applied = target;
        drop(applied);
        if let Some(previous) = previous {
            // The shutdown flushes what is pending: it blocks, off the runtime.
            let _ = tokio::task::spawn_blocking(move || shutdown(&previous)).await;
        }
    }

    fn build(&self, target: &ExportTarget) -> Result<Active, String> {
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
        let builders =
            self.exporters
                .attach(target, self.interval, self.status.clone(), builders)?;
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
        })
    }

    pub fn status(&self) -> TelemetryStatus {
        TelemetryStatus {
            exporting: self.active().is_some(),
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
        let active = self.active().ok_or(TestExportError::NotExporting)?;
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
