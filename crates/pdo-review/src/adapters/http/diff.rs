//! `GET /runs/{run_id}/diff` and `GET /runs/{run_id}/diff/structured`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::answer;
use super::dto::StructuredDiffQuery;
use crate::Review;

/// The Run's raw patch, as `text/plain`.
pub(super) async fn raw(State(review): State<Arc<Review>>, Path(run_id): Path<String>) -> Response {
    match review.run_diff(&run_id).await {
        Ok(diff) => (StatusCode::OK, diff).into_response(),
        Err(failure) => failure.into_response(),
    }
}

/// Files → hunks → lines for a pair of Run refs.
pub(super) async fn structured(
    State(review): State<Arc<Review>>,
    Path(run_id): Path<String>,
    Query(q): Query<StructuredDiffQuery>,
) -> Response {
    answer(
        review
            .structured_diff(&run_id, q.from.as_deref(), q.to.as_deref())
            .await,
    )
}
