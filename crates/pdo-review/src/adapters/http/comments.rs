//! The review comments' routes: the list and the gestures on it.

use std::sync::Arc;

use axum::extract::{Json, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pdo_kernel::actor::actor_from_headers;
use pdo_kernel::Actor;

use super::answer;
use super::dto::{
    ListReviewCommentsQuery, ReplyReviewCommentRequest, ResendReviewCommentsRequest,
    ReviewDecisionRequest, SendReviewCommentsRequest,
};
use crate::application::list_comments::ListComments;
use crate::application::reply::ReplyCommand;
use crate::domain::comments::decision::Decision;
use crate::Review;

/// `GET /runs/{run_id}/review/comments[?state=&from=&to=]`.
pub(super) async fn list(
    State(review): State<Arc<Review>>,
    Path(run_id): Path<String>,
    Query(q): Query<ListReviewCommentsQuery>,
) -> Response {
    let query = ListComments {
        state: q.state.as_deref(),
        from: q.from.as_deref(),
        to: q.to.as_deref(),
    };
    answer(review.list_comments(&run_id, query).await)
}

/// The node of the session the call comes from, by its session headers:
/// what names the author's role when an extension identified the caller as a
/// person (CORE-63).
fn session_node(headers: &HeaderMap) -> Option<String> {
    match actor_from_headers(headers) {
        Actor::Agent { node_id, .. } => Some(node_id),
        _ => None,
    }
}

/// `POST /runs/{run_id}/review/comments/{comment_id}/reply`.
pub(super) async fn reply(
    State(review): State<Arc<Review>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    Path((run_id, comment_id)): Path<(String, String)>,
    Json(req): Json<ReplyReviewCommentRequest>,
) -> Response {
    let command = ReplyCommand {
        text: req.text,
        resolved: req.resolved,
        author: req.author,
        session_node: session_node(&headers),
    };
    answer(review.reply(&actor, &run_id, &comment_id, command).await)
}

/// `POST /runs/{run_id}/review/comments/{comment_id}/resolve`.
pub(super) async fn resolve(
    State(review): State<Arc<Review>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    Path((run_id, comment_id)): Path<(String, String)>,
    body: Option<Json<ReviewDecisionRequest>>,
) -> Response {
    let session = session_node(&headers);
    decide(
        review,
        actor,
        session,
        run_id,
        comment_id,
        body,
        Decision::Resolve,
    )
    .await
}

/// `POST /runs/{run_id}/review/comments/{comment_id}/reopen`.
pub(super) async fn reopen(
    State(review): State<Arc<Review>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    Path((run_id, comment_id)): Path<(String, String)>,
    body: Option<Json<ReviewDecisionRequest>>,
) -> Response {
    let session = session_node(&headers);
    decide(
        review,
        actor,
        session,
        run_id,
        comment_id,
        body,
        Decision::Reopen,
    )
    .await
}

async fn decide(
    review: Arc<Review>,
    actor: Actor,
    session: Option<String>,
    run_id: String,
    comment_id: String,
    body: Option<Json<ReviewDecisionRequest>>,
    decision: Decision,
) -> Response {
    let by = body.map(|Json(b)| b).unwrap_or_default().by;
    answer(
        review
            .decide(
                &actor,
                &run_id,
                &comment_id,
                decision,
                by.as_deref(),
                session.as_deref(),
            )
            .await,
    )
}

/// `POST /runs/{run_id}/review/comments/send`.
pub(super) async fn send(
    State(review): State<Arc<Review>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
    Json(req): Json<SendReviewCommentsRequest>,
) -> Response {
    let drafts = req.comments.into_iter().map(Into::into).collect();
    match review.send_comments(&actor, &run_id, drafts).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(failure) => failure.into_response(),
    }
}

/// `POST /runs/{run_id}/review/comments/resend` (CORE-239).
pub(super) async fn resend(
    State(review): State<Arc<Review>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
    Json(req): Json<ResendReviewCommentsRequest>,
) -> Response {
    answer(review.resend_comments(&actor, &run_id, req.ids).await)
}
