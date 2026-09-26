//! `GET /audit` — the out-of-Run audit feed, newest first.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::error;
use crate::domain::audit::{AuditFilter, DEFAULT_AUDIT_LIMIT};
use crate::Platform;

#[derive(Deserialize)]
pub(super) struct AuditQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    target_kind: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default = "default_audit_limit")]
    limit: i64,
}

fn default_audit_limit() -> i64 {
    DEFAULT_AUDIT_LIMIT
}

pub(super) async fn feed(
    State(platform): State<Arc<Platform>>,
    Query(q): Query<AuditQuery>,
) -> Response {
    let filter = AuditFilter {
        from: q.from,
        to: q.to,
        target_kind: q.target_kind,
        target_id: q.target_id,
        limit: q.limit,
    };
    match platform.list_audit.execute(&filter).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("audit list: {e}"),
        ),
    }
}
