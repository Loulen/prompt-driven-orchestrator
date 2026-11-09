//! The review's routes: a Run's raw and structured diff, its review comments,
//! and the gestures on them (send, send again, reply, resolve, reopen). Each translates
//! the request and hands it to its use case; a gesture's use case receives
//! the Actor `identify` set. The composition root merges [`router`] behind
//! the `identify` middleware.

mod comments;
mod diff;
pub mod dto;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::application::failure::Failure;
use crate::Review;

/// The routes of the review context, with their state.
pub fn router(review: Arc<Review>) -> Router {
    Router::new()
        .route("/runs/{run_id}/diff", get(diff::raw))
        .route("/runs/{run_id}/diff/structured", get(diff::structured))
        .route("/runs/{run_id}/review/comments", get(comments::list))
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/reply",
            post(comments::reply),
        )
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/resolve",
            post(comments::resolve),
        )
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/reopen",
            post(comments::reopen),
        )
        .route("/runs/{run_id}/review/comments/send", post(comments::send))
        .route(
            "/runs/{run_id}/review/comments/resend",
            post(comments::resend),
        )
        .with_state(review)
}

fn status(code: u16) -> StatusCode {
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Failure::Text(code, body) => (status(code), body).into_response(),
            Failure::Json(code, body) => (status(code), Json(body)).into_response(),
            Failure::Refused(denied) => denied.into_response(),
        }
    }
}

/// The use case's answer, `200` with its JSON body, or its failure.
fn answer<T: serde::Serialize>(outcome: Result<T, Failure>) -> Response {
    match outcome {
        Ok(body) => Json(body).into_response(),
        Err(failure) => failure.into_response(),
    }
}
