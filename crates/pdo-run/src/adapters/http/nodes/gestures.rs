//! The node routes that change a Run: the agent's `done` / `fail` / `skip`,
//! the release of an interactive completion, and the canvas buttons (start,
//! stop, retry). Each hands its use case the Actor `identify` set.

use std::sync::Arc;

use axum::extract::{Json, Path, State};
use axum::response::Response;
use axum::Extension;
use pdo_kernel::Actor;

use super::requests::{
    NodeDoneRequest, NodeFailRequest, NodeSkipRequest, ReleaseNodeCompletionRequest,
};
use crate::application::complete_node::CompleteNode;
use crate::application::fail_node::FailNode;
use crate::application::release_completion::ReleaseCompletion;
use crate::application::retry_node::RetryNode;
use crate::application::skip_node::SkipNode;
use crate::application::start_node::StartNode;
use crate::application::stop_node::StopNode;
use crate::Runs;

/// `POST /runs/{run_id}/nodes/{node_id}/done`.
pub(in crate::adapters::http) async fn done(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
    body: Option<Json<NodeDoneRequest>>,
) -> Response {
    // Read `iter` and `auto` together before `body` is consumed. A legacy body
    // without the field deserialises `auto = false` ⇒ `Explicit`.
    let (iter, auto) = body
        .map(|Json(b)| (b.iter.unwrap_or(1), b.auto))
        .unwrap_or((1, false));
    let command = CompleteNode {
        run_id,
        node_id,
        iter,
        auto,
    };
    runs.complete_node(&actor, command).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/release`.
pub(in crate::adapters::http) async fn release(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
    body: Option<Json<ReleaseNodeCompletionRequest>>,
) -> Response {
    let iter = body.and_then(|Json(body)| body.iter).unwrap_or(1);
    let command = ReleaseCompletion {
        run_id,
        node_id,
        iter,
    };
    runs.release_completion(&actor, command).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/fail`.
pub(in crate::adapters::http) async fn fail(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(req): Json<NodeFailRequest>,
) -> Response {
    let command = FailNode {
        run_id,
        node_id,
        iter: req.iter,
        reason: req.reason,
    };
    runs.fail_node(&actor, command).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/skip`.
pub(in crate::adapters::http) async fn skip(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(req): Json<NodeSkipRequest>,
) -> Response {
    let command = SkipNode {
        run_id,
        node_id,
        iter: req.iter,
        reason: req.reason,
    };
    runs.skip_node(&actor, command).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/start` — the canvas's Start button.
pub(in crate::adapters::http) async fn start(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Response {
    runs.start_node(&actor, StartNode { run_id, node_id }).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/stop` — the canvas's Stop button.
pub(in crate::adapters::http) async fn stop(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Response {
    runs.stop_node(&actor, StopNode { run_id, node_id }).await
}

/// `POST /runs/{run_id}/nodes/{node_id}/retry` — the canvas's Retry button.
pub(in crate::adapters::http) async fn retry(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Response {
    runs.retry_node(&actor, RetryNode { run_id, node_id }).await
}
