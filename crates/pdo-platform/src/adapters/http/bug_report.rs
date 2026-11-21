//! The Destinations de bug report on HTTP:
//!
//! - `GET /bug-report/destinations`: what the extensions offer the caller,
//!   and the last destination it chose (`github` for the core's);
//! - `POST /bug-report`: a report sent to an extension's destination;
//!   `200` [`BugReportSent`], `404 unknown_destination`, `400
//!   bug_report_refused` (no description, prompts toward a destination that
//!   refuses them, a screenshot that is no image or too large), `502
//!   bug_report_not_sent` with the extension's reason;
//! - `PUT /bug-report/last` `{destination}`: remember the core's destination
//!   (or an offered one) as the caller's last choice; `204`.
//!
//! ROUTER TRAP: `/bug-report` is a top-level prefix of the vite dev proxy
//! whitelist (`frontend/vite.config.ts`).

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pdo_extension_api::{BugReportFields, Screenshot};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::bug_report::{offered_id, OutgoingReport};
use crate::domain::bug_report::BugReportRefusal;
use crate::Platform;

/// One destination an extension offers, as « Send to » lists it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BugReportDestinationView {
    /// `<extension>/<id>`.
    pub id: String,
    pub label: String,
    pub accepts_prompts: bool,
    /// An SVG the extension serves; `null` draws the label's first letter.
    pub icon: Option<String>,
    /// The extension's id.
    pub extension: String,
    /// The extension's name: the header of its group.
    pub extension_name: String,
}

/// `GET /bug-report/destinations`.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BugReportDestinations {
    /// The extensions' destinations, in registration order; the core's own
    /// (`github`) is not listed: the browser opens it.
    pub destinations: Vec<BugReportDestinationView>,
    /// The caller's last choice (`github`, or one of `destinations`); `null`
    /// while it never chose, or when its choice is no longer offered.
    pub last: Option<String>,
}

/// A screenshot, as the window sends it.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct BugReportScreenshot {
    /// `image/png`.
    pub media_type: String,
    /// The bytes, base64 (standard alphabet, padded).
    pub data: String,
}

/// `POST /bug-report`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SendBugReport {
    /// `<extension>/<id>`.
    pub destination: String,
    /// The report as markdown, as the Preview shows it.
    pub text: String,
    pub fields: BugReportFields,
    #[serde(default)]
    #[ts(optional)]
    pub screenshot: Option<BugReportScreenshot>,
}

/// `PUT /bug-report/last`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RememberBugReportDestination {
    pub destination: String,
}

fn refusal(refusal: BugReportRefusal) -> Response {
    let (status, slug) = match &refusal {
        BugReportRefusal::UnknownDestination(_) => (StatusCode::NOT_FOUND, "unknown_destination"),
        BugReportRefusal::NotReceived(_) => (StatusCode::BAD_GATEWAY, "bug_report_not_sent"),
        BugReportRefusal::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage"),
        BugReportRefusal::ScreenshotTooLarge => {
            (StatusCode::PAYLOAD_TOO_LARGE, "bug_report_refused")
        }
        _ => (StatusCode::BAD_REQUEST, "bug_report_refused"),
    };
    let body = serde_json::json!({ "error": slug, "message": refusal.to_string() });
    (status, Json(body)).into_response()
}

pub(super) async fn destinations(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
) -> Json<BugReportDestinations> {
    let destinations = platform.bug_reports.destinations(&actor).await;
    Json(BugReportDestinations {
        destinations: destinations
            .offered
            .iter()
            .map(|o| BugReportDestinationView {
                id: offered_id(o),
                label: o.destination.label.clone(),
                accepts_prompts: o.destination.accepts_prompts,
                icon: o.destination.icon.clone(),
                extension: o.extension.clone(),
                extension_name: o.extension_name.clone(),
            })
            .collect(),
        last: destinations.last,
    })
}

pub(super) async fn send(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
    Json(request): Json<SendBugReport>,
) -> Response {
    let screenshot = match request.screenshot {
        None => None,
        Some(shot) => match BASE64.decode(shot.data.as_bytes()) {
            Ok(bytes) => Some(Screenshot::new(shot.media_type, bytes)),
            Err(e) => return refusal(BugReportRefusal::UnreadableScreenshot(e.to_string())),
        },
    };
    let report = OutgoingReport {
        destination: request.destination,
        text: request.text,
        fields: request.fields,
        screenshot,
    };
    match platform.bug_reports.send(&actor, report).await {
        Ok(sent) => Json(sent).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn remember(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
    Json(request): Json<RememberBugReportDestination>,
) -> Response {
    match platform
        .bug_reports
        .remember(&actor, &request.destination)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refusal(e),
    }
}
