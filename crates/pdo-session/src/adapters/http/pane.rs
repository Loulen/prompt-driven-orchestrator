//! `GET /runs/{run_id}/nodes/{node_id}/pane` — what the UI shows of a node
//! session.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::run_lookup;
use crate::application::read_pane::ReadPaneError;
use crate::Session;

#[derive(Serialize, TS)]
#[ts(export)]
pub struct PaneResponse {
    content: String,
    session_name: String,
    resumed: bool,
    stale: bool,
    /// Provenance of `content`: `"live"`, `"resumed"` (a dead latest-iter session
    /// was re-attached), or `"snapshot"` (the persisted post-mortem pane).
    source: &'static str,
}

#[derive(Deserialize)]
pub(super) struct IterQuery {
    #[serde(default = "default_iter")]
    iter: i64,
}

fn default_iter() -> i64 {
    1
}

pub(super) async fn read(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    match session
        .read_pane()
        .execute(&actor, &run_id, &node_id, query.iter)
        .await
    {
        Ok((session_name, pane)) => Json(PaneResponse {
            content: pane.content,
            session_name,
            resumed: pane.resumed,
            stale: pane.stale,
            source: pane.source.as_str(),
        })
        .into_response(),
        Err(ReadPaneError::Run(lookup)) => run_lookup(lookup),
        Err(ReadPaneError::NodeNotFound) => {
            (StatusCode::NOT_FOUND, "node not found in run").into_response()
        }
        Err(ReadPaneError::Refused(denied)) => denied.into_response(),
    }
}
