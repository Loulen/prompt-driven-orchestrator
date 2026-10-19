//! The OTLP exporters over HTTP (protobuf), the daemon's: the SDK's periodic
//! reader for the metrics and its batch processor for the spans, both posting
//! through the daemon's own HTTP client. Every post reports its answer to the
//! export status.
//!
//! The SDK exports from its own threads; the client hands each post to the
//! daemon's runtime, so a collector that hangs or refuses never holds a Run.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};
use opentelemetry_otlp::{MetricExporter, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::PeriodicReader;

use crate::application::ports::{Builders, Exporters};
use crate::application::status::ExportStatus;
use crate::domain::setting::{ExportTarget, Signal};

/// A post that takes longer is abandoned and reported as such.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The exporters of the daemon, over its runtime.
pub struct OtlpExporters {
    runtime: tokio::runtime::Handle,
}

impl OtlpExporters {
    /// Over the runtime of the caller.
    pub fn current() -> Self {
        Self {
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

impl Exporters for OtlpExporters {
    fn attach(
        &self,
        target: &ExportTarget,
        interval: Duration,
        status: Arc<ExportStatus>,
        builders: Builders,
    ) -> Result<Builders, String> {
        let client = Arc::new(Client {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .map_err(|e| e.to_string())?,
            runtime: self.runtime.clone(),
            status,
        });
        let headers: HashMap<String, String> = target.headers.clone().into_iter().collect();
        let metrics = MetricExporter::builder()
            .with_http()
            .with_endpoint(target.url(Signal::Metrics))
            .with_timeout(TIMEOUT)
            .with_headers(headers.clone())
            .with_shared_http_client(client.clone())
            .build()
            .map_err(|e| e.to_string())?;
        let spans = SpanExporter::builder()
            .with_http()
            .with_endpoint(target.url(Signal::Traces))
            .with_timeout(TIMEOUT)
            .with_headers(headers)
            .with_shared_http_client(client)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Builders {
            meters: builders.meters.with_reader(
                PeriodicReader::builder(metrics)
                    .with_interval(interval)
                    .build(),
            ),
            tracers: builders.tracers.with_batch_exporter(spans),
        })
    }
}

/// The SDK's HTTP client: the daemon's reqwest, on the daemon's runtime.
#[derive(Debug)]
struct Client {
    http: reqwest::Client,
    runtime: tokio::runtime::Handle,
    status: Arc<ExportStatus>,
}

#[async_trait::async_trait]
impl HttpClient for Client {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let signal = signal_of(request.uri().path());
        let (parts, body) = request.into_parts();
        let mut post = self
            .http
            .request(parts.method, parts.uri.to_string())
            .body(body);
        for (name, value) in &parts.headers {
            post = post.header(name, value);
        }
        let status = self.status.clone();
        // The SDK's thread waits on the runtime's task, never on a reactor of
        // its own.
        let answer = self
            .runtime
            .spawn(async move {
                let response = post.send().await;
                let at = chrono::Utc::now();
                match response {
                    Ok(response) => {
                        status.answered(signal, response.status().as_u16(), at);
                        let code = response.status().as_u16();
                        let body = response.bytes().await.unwrap_or_default();
                        Ok((code, body))
                    }
                    Err(error) => {
                        let reason = describe(&error);
                        status.unreachable(signal, &reason, at);
                        Err(reason)
                    }
                }
            })
            .await
            .map_err(|e| HttpError::from(e.to_string()))?;
        let (code, body) = answer.map_err(HttpError::from)?;
        Response::builder()
            .status(code)
            .body(body)
            .map_err(|e| HttpError::from(e.to_string()))
    }
}

fn signal_of(path: &str) -> &'static str {
    if path.ends_with(Signal::Traces.path()) {
        "traces"
    } else {
        "metrics"
    }
}

/// Why a post got no answer, short enough for the page.
fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "timed out".to_string()
    } else if error.is_connect() {
        "connection refused or unreachable".to_string()
    } else {
        error.to_string()
    }
}
