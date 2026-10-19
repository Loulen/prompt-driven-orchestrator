//! `/metrics` in the Prometheus text format: the SDK's pull reader over a
//! registry of its own, rendered at each scrape. Plugged into the same meters
//! as the OTLP export, it holds the same measures; the names follow the
//! Prometheus conventions (`pdo.runs.started` reads `pdo_runs_started_total`,
//! a duration gets its `_seconds`), the resource goes on `target_info`.

use std::sync::Arc;

use opentelemetry_sdk::metrics::MeterProviderBuilder;
use prometheus::{Encoder as _, Registry, TextEncoder};

use crate::application::ports::{MetricsEndpoint, Scrape};

/// The `Content-Type` of a scrape.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// A fresh registry per set of providers: a rebuilt provider never reads the
/// measures of the one it replaced.
pub struct PrometheusEndpoint;

impl MetricsEndpoint for PrometheusEndpoint {
    fn attach(
        &self,
        meters: MeterProviderBuilder,
    ) -> Result<(MeterProviderBuilder, Arc<dyn Scrape>), String> {
        let registry = Registry::new();
        let reader = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()
            .map_err(|e| e.to_string())?;
        Ok((meters.with_reader(reader), Arc::new(Registered(registry))))
    }
}

struct Registered(Registry);

impl Scrape for Registered {
    fn render(&self) -> Result<String, String> {
        let mut text = Vec::new();
        TextEncoder::new()
            .encode(&self.0.gather(), &mut text)
            .map_err(|e| e.to_string())?;
        String::from_utf8(text).map_err(|e| e.to_string())
    }
}
