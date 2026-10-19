//! The node routes that only read: a node's prompt, its inputs and outputs, its
//! diff, its transcript, what a retry would invalidate, and the long-poll on its
//! child runs.
//! None asks the Autorisation d'opération.

use std::sync::Arc;

use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::application::children_wait::{children_wait, ChildrenWaitQuery};
use crate::application::session_claim::session_claim_from_headers;

use crate::adapters::http::nodes::requests::{IterQuery, TranscriptQuery};
use crate::application::events::load_projected;
use crate::application::node::io as node_io_resolver;
use crate::application::node::launch::snapshot_isolation;
use crate::application::paths::archived_or_live_artifacts_dir;
use crate::application::paths::archived_or_live_pipeline_yaml;
use crate::application::paths::effective_repo_root;
use crate::Runs;
use pdo_pipeline::domain::pipeline;

pub(in crate::adapters::http) async fn node_prompt(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    let iter = query.iter;

    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    if !run_state.nodes.contains_key(&node_id) {
        return (StatusCode::NOT_FOUND, "node not found in run").into_response();
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let working_dir = pdo_session::domain::names::working_dir_for_node(
        &repo_root,
        &run_id,
        &node_id,
        iter,
        // #653: the prompt file lives wherever the NodeRun works. Read from the
        // snapshot: this endpoint holds no event log, and a started iteration's
        // frozen value equals it unless the document was edited mid-run.
        snapshot_isolation(&run_state, &node_id),
    );

    let prompt_path = working_dir
        .join(".pdo")
        .join("prompts")
        .join(format!("{node_id}-iter-{iter}.md"));

    match std::fs::read_to_string(&prompt_path) {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/markdown")],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "prompt file not found").into_response(),
    }
}

pub(in crate::adapters::http) async fn node_io(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    let iter = query.iter;

    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    if !run_state.nodes.contains_key(&node_id)
        && !run_state.node_defs.iter().any(|nd| nd.id == node_id)
    {
        return (StatusCode::NOT_FOUND, "node not found in run").into_response();
    }

    // An archived run reads its pipeline + artifacts from the durable store, a live
    // run from its worktree. `node_io_resolver::resolve` needs a real parsed
    // `PipelineDef` — it reads port_type/repeated, which the projection lacks.
    let yaml_path = archived_or_live_pipeline_yaml(&state, &run_state, &run_id);
    let pipeline = match std::fs::read_to_string(&yaml_path)
        .ok()
        .and_then(|y| pipeline::parse_pipeline(&y).ok())
    {
        Some(r) => r.pipeline,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not load run pipeline",
            )
                .into_response();
        }
    };

    let artifacts_dir = archived_or_live_artifacts_dir(&state, &run_state, &run_id);

    // The resolver reads `repeated` pools through the projection, so a failed
    // iteration's artifact on disk is never surfaced as resolvable.
    let io = node_io_resolver::resolve(&pipeline, &artifacts_dir, &node_id, iter, &run_state);
    Json(io).into_response()
}

/// `GET /runs/{run_id}/nodes/{node_id}/diff`.
pub(in crate::adapters::http) async fn node_diff(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    crate::application::node_diff::node_diff(&state, run_id, node_id).await
}

/// `GET /runs/{run_id}/nodes/{node_id}/transcript?iter=&after=&limit=`.
pub(in crate::adapters::http) async fn node_transcript(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<TranscriptQuery>,
) -> Response {
    crate::application::node::transcript::page(
        &state,
        &run_id,
        &node_id,
        query.iter,
        query.after,
        query.limit,
    )
    .await
}

/// `GET /runs/{run_id}/nodes/{node_id}/transcript/tools/{index}?iter=`.
pub(in crate::adapters::http) async fn node_transcript_tool(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id, index)): AxumPath<(String, String, usize)>,
    Query(query): Query<IterQuery>,
) -> Response {
    crate::application::node::transcript::tool_detail(&state, &run_id, &node_id, query.iter, index)
        .await
}

/// `GET /runs/{run_id}/nodes/{node_id}/retry/preview`.
pub(in crate::adapters::http) async fn retry_preview(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    crate::application::retry_node::retry_preview(&state, run_id, node_id).await
}

/// `GET /runs/{run_id}/nodes/{node_id}/children/wait` — `pdo run wait`'s
/// long-poll. The session headers name the node session asking.
pub(in crate::adapters::http) async fn node_children_wait(
    State(state): State<Arc<Runs>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Query(query): Query<ChildrenWaitQuery>,
) -> Response {
    let claim = session_claim_from_headers(&headers);
    children_wait(&state, run_id, node_id, claim, query).await
}
