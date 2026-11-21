//! The Destinations de bug report of the test extension (API 0.28): the
//! core's fixture for `bug_report_destinations` and `receive_bug_report`.
//!
//! - It offers two destinations: [`INBOX`] (« Test inbox », accepts the
//!   prompts, its icon the extension's) and [`BOARD`] (« Test board », refuses
//!   them); to the identified Actor [`GUEST`], only the board: the list
//!   depends on the Actor.
//! - Each report it receives is kept, numbered from 1, and answered
//!   `BUG-<n>`, its link `/ext/test/bug-reports/<n>`: `GET` there answers the
//!   report (destination, title, text, fields, the Actor who sent it, the
//!   screenshot's media type and size), `GET …/<n>/screenshot` its bytes, `GET
//!   /ext/test/bug-reports` them all.
//! - `PUT /ext/test/bug-reports/refusal` `{"reason": "…"}` has it refuse the
//!   next reports with that reason (`{"reason": null}` lifts it): the window
//!   shows its failure.

use std::sync::{Arc, Mutex};

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::{header, StatusCode};
use pdo::extension_api::axum::response::IntoResponse;
use pdo::extension_api::axum::routing::{get, put};
use pdo::extension_api::axum::{Json, Router};
use pdo::extension_api::{Actor, BugReport, BugReportDestination, BugReportSent};
use serde_json::{json, Value};

use super::{actor_label, ID};

/// The destination that accepts the prompts.
pub const INBOX: &str = "inbox";
/// The destination that refuses them.
pub const BOARD: &str = "board";
/// The identified Actor offered the board alone.
pub const GUEST: &str = "guest";

/// One report the test extension received.
#[derive(Debug, Clone)]
pub struct Received {
    pub actor: Actor,
    pub report: BugReport,
}

/// What the test extension received, and whether it refuses.
#[derive(Clone, Default)]
pub struct BugReports {
    received: Arc<Mutex<Vec<Received>>>,
    refusal: Arc<Mutex<Option<String>>>,
}

impl BugReports {
    /// Every report received, in order.
    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }

    /// Refuse the next reports with `reason` (`None`: receive them).
    pub fn refuse(&self, reason: Option<String>) {
        *self.refusal.lock().unwrap() = reason;
    }

    pub(super) fn destinations(&self, actor: &Actor) -> Vec<BugReportDestination> {
        let board = BugReportDestination::new(BOARD, "Test board");
        if matches!(actor, Actor::Identified { id, .. } if id == GUEST) {
            return vec![board];
        }
        vec![
            BugReportDestination::new(INBOX, "Test inbox")
                .accepting_prompts()
                .with_icon(format!("/ext/{ID}/icon.svg")),
            board,
        ]
    }

    pub(super) fn receive(
        &self,
        actor: &Actor,
        report: &BugReport,
    ) -> Result<BugReportSent, String> {
        if let Some(reason) = self.refusal.lock().unwrap().clone() {
            return Err(reason);
        }
        let mut received = self.received.lock().unwrap();
        received.push(Received {
            actor: actor.clone(),
            report: report.clone(),
        });
        let n = received.len();
        Ok(BugReportSent::new(
            format!("BUG-{n}"),
            format!("/ext/{ID}/bug-reports/{n}"),
        ))
    }
}

fn view(n: usize, received: &Received) -> Value {
    let report = &received.report;
    json!({
        "reference": format!("BUG-{n}"),
        "actor": actor_label(&received.actor),
        "destination": report.destination,
        "title": report.title,
        "text": report.text,
        "fields": report.fields,
        "screenshot": report.screenshot.as_ref().map(|s| json!({
            "media_type": s.media_type,
            "size": s.bytes.len(),
        })),
    })
}

pub(super) fn routes(reports: BugReports) -> Router {
    let all = reports.clone();
    let one = reports.clone();
    let shot = reports.clone();
    Router::new()
        .route(
            "/bug-reports",
            get(move || {
                let received = all.received();
                async move {
                    let views: Vec<Value> = received
                        .iter()
                        .enumerate()
                        .map(|(i, r)| view(i + 1, r))
                        .collect();
                    Json(json!(views))
                }
            }),
        )
        .route(
            "/bug-reports/refusal",
            put(move |Json(body): Json<Value>| {
                reports.refuse(body["reason"].as_str().map(str::to_string));
                async { StatusCode::NO_CONTENT }
            }),
        )
        .route(
            "/bug-reports/{n}",
            get(move |Path(n): Path<usize>| {
                let found = n
                    .checked_sub(1)
                    .and_then(|i| one.received().get(i).cloned());
                async move {
                    match found {
                        Some(r) => Json(view(n, &r)).into_response(),
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }),
        )
        .route(
            "/bug-reports/{n}/screenshot",
            get(move |Path(n): Path<usize>| {
                let found = n
                    .checked_sub(1)
                    .and_then(|i| shot.received().get(i).cloned())
                    .and_then(|r| r.report.screenshot);
                async move {
                    match found {
                        Some(s) => {
                            ([(header::CONTENT_TYPE, s.media_type)], s.bytes).into_response()
                        }
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }),
        )
}
