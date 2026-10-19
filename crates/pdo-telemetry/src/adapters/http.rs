//! The telemetry's routes and the layer that measures the daemon's requests.
//!
//! - `GET /telemetry/status`: whether the export runs, the last export's
//!   answer, what is sent.
//! - `POST /telemetry/test-export`: export now (a test span included), answer
//!   the status that followed; `409` while nothing exports.
//! - [`measure`]: the latency of every routed request, and its span, labelled
//!   by its route pattern (`/runs/{id}`), never by the path it was called on;
//!   the telemetry's own routes are left out.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use opentelemetry::trace::{Span as _, SpanKind, Status, Tracer as _};
use opentelemetry::KeyValue;
use pdo_kernel::Actor;

use crate::application::hub::{Telemetry, TestExportError};

pub fn router(telemetry: Arc<Telemetry>) -> Router {
    Router::new()
        .route("/telemetry/status", get(status))
        .route("/telemetry/test-export", post(test_export))
        .with_state(telemetry)
}

async fn status(State(telemetry): State<Arc<Telemetry>>) -> Response {
    Json(telemetry.status()).into_response()
}

async fn test_export(
    State(telemetry): State<Arc<Telemetry>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match telemetry.test_export(&actor).await {
        Ok(status) => Json(status).into_response(),
        Err(TestExportError::Refused(denied)) => denied.into_response(),
        Err(TestExportError::NotExporting) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "the OTLP export is off, or names no collector",
            })),
        )
            .into_response(),
    }
}

/// The layer over the daemon's routes: one latency measure and one span per
/// request while the export runs, nothing otherwise.
pub async fn measure(
    State(telemetry): State<Arc<Telemetry>>,
    request: Request,
    next: Next,
) -> Response {
    // The Telemetry page polls its own status: measuring it would only
    // measure the page.
    let Some(active) = telemetry
        .active()
        .filter(|_| !request.uri().path().starts_with("/telemetry/"))
    else {
        return next.run(request).await;
    };
    // The static files of the interface have no route: one label for them all.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "static".to_string());
    let method = request.method().as_str().to_string();
    let started = Instant::now();
    let mut span = active
        .tracer
        .span_builder(format!("{method} {route}"))
        .with_kind(SpanKind::Server)
        .with_start_time(SystemTime::now())
        .with_attributes([
            KeyValue::new("http.request.method", method.clone()),
            KeyValue::new("http.route", route.clone()),
        ])
        .start(&active.tracer);
    let response = next.run(request).await;
    let status = response.status().as_u16();
    span.set_attribute(KeyValue::new(
        "http.response.status_code",
        i64::from(status),
    ));
    if status >= 500 {
        span.set_status(Status::error(status.to_string()));
    }
    span.end();
    active.instruments.http_duration.record(
        started.elapsed().as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.route", route),
            KeyValue::new("http.response.status_code", i64::from(status)),
        ],
    );
    response
}
