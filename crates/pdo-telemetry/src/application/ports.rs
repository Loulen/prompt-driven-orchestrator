//! The ports of the telemetry context: what the hub needs from the outside
//! world, as traits the composition root plugs adapters into.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use opentelemetry_sdk::metrics::{MeterProviderBuilder, SdkMeterProvider};
use opentelemetry_sdk::trace::{SdkTracerProvider, TracerProviderBuilder};

use super::status::ExportStatus;
use crate::domain::attributes::RunFacts;
use crate::domain::cost::CostSlice;
use crate::domain::setting::{ExportTarget, TelemetrySetting};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The telemetry setting, every tier resolved, read fresh at each call.
pub trait TelemetrySettingSource: Send + Sync {
    fn setting(&self) -> BoxFuture<'_, TelemetrySetting>;
}

/// What a Run's measures are attached to: the Projet, the repository named by
/// its remote, git, the pipeline and the harness, and who launched it, which
/// the recorder sends only with « include the Actor ».
pub trait RunFactsSource: Send + Sync {
    fn facts<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, RunFacts>;
}

/// A Run's cost and tokens, read once it ended.
pub trait RunCosts: Send + Sync {
    fn slices<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<CostSlice>>;
}

/// The agent sessions alive now, or `None` when they cannot be counted.
pub trait LiveSessions: Send + Sync {
    fn live(&self) -> BoxFuture<'_, Option<i64>>;
}

/// The providers the measures go through.
pub struct Providers {
    pub meters: SdkMeterProvider,
    pub tracers: SdkTracerProvider,
}

/// The SDK's builders, already carrying the instance's resource, that each
/// channel plugs its reader and its span exporter into. The hub owns the
/// providers they build: a channel adds a destination, never the measures.
pub struct Builders {
    pub meters: MeterProviderBuilder,
    pub tracers: TracerProviderBuilder,
}

/// The channel that pushes to a collector: the OTLP exporters in the daemon,
/// in-memory ones in the tests. Every export reports its outcome to `status`.
pub trait Exporters: Send + Sync {
    fn attach(
        &self,
        target: &ExportTarget,
        interval: Duration,
        status: Arc<ExportStatus>,
        builders: Builders,
    ) -> Result<Builders, String>;
}

/// The channel read on `/metrics`: a pull reader plugged into the meters, and
/// what renders what it holds at each scrape.
pub trait MetricsEndpoint: Send + Sync {
    fn attach(
        &self,
        meters: MeterProviderBuilder,
    ) -> Result<(MeterProviderBuilder, Arc<dyn Scrape>), String>;
}

/// One scrape of `/metrics`: the measures as they stand now, in the
/// endpoint's text format.
pub trait Scrape: Send + Sync {
    fn render(&self) -> Result<String, String>;
}
