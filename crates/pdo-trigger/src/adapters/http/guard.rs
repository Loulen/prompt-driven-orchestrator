//! `POST /triggers/guard/test` — dry-run a Trigger guard command.

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Serialize;
use ts_rs::TS;

use super::refusal;
use crate::application::test_guard::GuardTestRequest;
use crate::domain::fire_decision::GuardResult;
use crate::Triggers;

/// Response of `POST /triggers/guard/test`: a 1:1 projection of
/// [`fire_decision::GuardResult`]. The would-fire / would-skip verdict is composed
/// client-side from `outcome`. `exit_code` is `None` on error (spawn failure/timeout).
#[derive(Debug, Serialize, TS)]
#[ts(export, rename = "TestGuardResponse")]
pub struct GuardTestResponse {
    outcome: &'static str,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    detail: Option<String>,
}

impl From<GuardResult> for GuardTestResponse {
    fn from(r: GuardResult) -> Self {
        match r {
            GuardResult::Pass { stdout, stderr } => Self {
                outcome: "pass",
                stdout,
                stderr,
                exit_code: Some(0),
                detail: None,
            },
            GuardResult::Skip {
                stdout,
                stderr,
                exit_code,
            } => Self {
                outcome: "skip",
                stdout,
                stderr,
                exit_code,
                detail: None,
            },
            GuardResult::Error { detail, .. } => Self {
                outcome: "error",
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
                detail: Some(detail),
            },
        }
    }
}

pub(super) async fn test(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<GuardTestRequest>,
) -> Response {
    match triggers.test_guard(&actor, req).await {
        Ok(result) => Json(GuardTestResponse::from(result)).into_response(),
        Err(e) => refusal(e),
    }
}
