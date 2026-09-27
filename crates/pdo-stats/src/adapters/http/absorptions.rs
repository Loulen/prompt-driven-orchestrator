//! `/stats/absorptions` (#888, ADR-0077): the list, the Combine and the ✕ of
//! the members list. Instance configuration, applied on read by the three
//! `/stats/*` folds.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use pdo_platform::adapters::http::OriginHint;
use serde::Deserialize;

use super::error;
use crate::application::absorptions::combine::CombineError;
use crate::application::absorptions::uncombine::{UncombineCommand, UncombineError};
use crate::domain::absorption::request::CombineRequest;
use crate::Stats;

/// `GET /stats/absorptions`.
pub(super) async fn list(State(stats): State<Arc<Stats>>) -> Response {
    match stats.list_absorptions().execute().await {
        Ok(list) => Json(list).into_response(),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("stats absorptions failed: {e}"),
        ),
    }
}

/// `POST /stats/absorptions` — create or extend an absorption. Answers the
/// whole list, as it stands after the write.
pub(super) async fn combine(
    State(stats): State<Arc<Stats>>,
    Extension(actor): Extension<Actor>,
    hint: OriginHint,
    Json(body): Json<CombineRequest>,
) -> Response {
    match stats.combine().execute(&actor, hint.as_hint(), body).await {
        Ok(list) => Json(list).into_response(),
        Err(CombineError::Refused(denied)) => denied.into_response(),
        Err(CombineError::Invalid(message)) => error(StatusCode::BAD_REQUEST, message),
        Err(CombineError::Failed(message)) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct UncombineQuery {
    dimension: String,
    #[serde(default)]
    scope: String,
    member: String,
}

/// `DELETE /stats/absorptions/member?dimension=&scope=&member=` — the ✕ of the
/// members list. The key travels in the query string: a Pipeline key is a
/// file-name stem or a free name, never safe as a path segment.
pub(super) async fn uncombine(
    State(stats): State<Arc<Stats>>,
    Extension(actor): Extension<Actor>,
    hint: OriginHint,
    Query(q): Query<UncombineQuery>,
) -> Response {
    let command = UncombineCommand {
        dimension: q.dimension,
        scope: q.scope,
        member: q.member,
    };
    match stats
        .uncombine()
        .execute(&actor, hint.as_hint(), command)
        .await
    {
        Ok(list) => Json(list).into_response(),
        Err(UncombineError::Refused(denied)) => denied.into_response(),
        Err(UncombineError::Invalid(message)) => error(StatusCode::BAD_REQUEST, message),
        Err(UncombineError::NotCombined) => {
            error(StatusCode::NOT_FOUND, "this row is not combined")
        }
        Err(UncombineError::Failed(message)) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
