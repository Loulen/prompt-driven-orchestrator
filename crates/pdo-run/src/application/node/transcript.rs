//! A node iteration's **Transcript** (CONTEXT.md): which file holds it, then a
//! page of its normalized entries or one tool call's detail, read through the
//! harness context's reader (`pdo_harness::adapters::transcript_reader`).
//!
//! The file is the one the iteration's **frozen** launch names (ADR-0046): its
//! harness, its pinned session id, where it worked (its frozen isolation), and
//! the store root of that harness for this Run (the staged home while a
//! sandboxed Run lives, the host store otherwise) — the same resolution the
//! liveness sweep and the cost fold read.

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pdo_harness::adapters::transcript_reader::TranscriptReader;
use pdo_harness::transcript::TranscriptPage;
use pdo_sandbox::adapters::staging::stores::{transcripts_root, HarnessStores};

use crate::application::events::load_projected;
use crate::application::node::launch::{
    find_launch_claude_root, find_launch_harness, find_launch_node_started_payload,
    find_launch_session_id, find_node_type, node_run_isolation,
};
use crate::application::paths::effective_repo_root;
use crate::application::sandbox::sandbox_home_roots;
use crate::domain::{event, state};
use crate::Runs;

/// The largest page a caller may ask for.
pub const MAX_PAGE: usize = 200;

/// Where a node iteration's transcript is, as far as PDO can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptSource {
    /// PDO reads no transcript here: a node that runs no agent, or a harness
    /// with no reader. `harness` names it when known.
    Unavailable { harness: Option<String> },
    /// A readable harness whose file is not there (yet).
    NotWritten { harness: Option<String> },
    /// The file to read, with the harness that wrote it.
    File { harness: String, path: PathBuf },
}

/// The store roots of a Run, injected so the resolution stays path math.
pub struct StoreRoots {
    pub home_root: PathBuf,
    pub sandbox_root: PathBuf,
}

/// Resolve the transcript of `(node_id, iter)` from the Run's events.
pub fn locate(
    events: &[event::Event],
    run_state: &state::RunState,
    repo_root: &Path,
    node_id: &str,
    iter: i64,
    roots: &StoreRoots,
) -> TranscriptSource {
    if matches!(find_node_type(run_state, node_id), Some(kind) if kind != "agent") {
        return TranscriptSource::Unavailable { harness: None };
    }
    if find_launch_node_started_payload(events, node_id, iter).is_none() {
        let harness = run_state
            .nodes
            .get(node_id)
            .and_then(|node| node.harness.clone());
        return match harness {
            Some(h) if !pdo_harness::probes::can_read_transcript(&h) => {
                TranscriptSource::Unavailable { harness: Some(h) }
            }
            harness => TranscriptSource::NotWritten { harness },
        };
    }
    // The floor of a launch that named no harness (ADR-0046).
    let harness = find_launch_harness(events, node_id, iter)
        .unwrap_or_else(|| pdo_harness::registry::CLAUDE.to_string());
    if !pdo_harness::probes::can_read_transcript(&harness) {
        return TranscriptSource::Unavailable {
            harness: Some(harness),
        };
    }
    let run_id = run_state.run_id.as_str();
    let working_dir = if node_run_isolation(events, run_state, node_id, iter) {
        pdo_workspace::adapters::git::worktrees::sub_worktree_path(repo_root, run_id, node_id, iter)
    } else {
        pdo_workspace::adapters::git::worktrees::worktree_dir_for_run(repo_root, run_id)
    };
    let sandboxed = !run_state.sandbox.is_off();
    let claude_root = find_launch_claude_root(events, node_id, iter).unwrap_or_else(|| {
        transcripts_root(sandboxed, run_id, &roots.home_root, &roots.sandbox_root)
    });
    let stores = HarnessStores::for_run(sandboxed, run_id, &roots.home_root, &roots.sandbox_root);
    let session_id = find_launch_session_id(events, node_id, iter);
    match pdo_harness::adapters::transcripts::resolve_transcript(
        &harness,
        stores.root_for(&harness, &claude_root),
        &working_dir,
        session_id.as_deref(),
    ) {
        Some(path) => TranscriptSource::File { harness, path },
        None => TranscriptSource::NotWritten {
            harness: Some(harness),
        },
    }
}

/// `GET /runs/{run_id}/nodes/{node_id}/transcript`: a page of the iteration's
/// transcript, from entry `after`, at most `limit` entries.
pub async fn page(
    state: &Runs,
    run_id: &str,
    node_id: &str,
    iter: i64,
    after: usize,
    limit: usize,
) -> Response {
    let source = match source(state, run_id, node_id, iter).await {
        Ok(source) => source,
        Err(response) => return response,
    };
    let limit = limit.clamp(1, MAX_PAGE);
    let page = match source {
        TranscriptSource::Unavailable { harness } => TranscriptPage::unavailable(harness),
        TranscriptSource::NotWritten { harness } => TranscriptPage::empty(harness),
        TranscriptSource::File { harness, path } => {
            let read = tokio::task::spawn_blocking(move || {
                TranscriptReader::shared()
                    .page(&harness, &path, after, limit)
                    .or_else(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            Ok(TranscriptPage::empty(Some(harness)))
                        } else {
                            Err(e)
                        }
                    })
            })
            .await;
            match read {
                Ok(Ok(page)) => page,
                Ok(Err(e)) => return internal(format!("cannot read the transcript: {e}")),
                Err(e) => return internal(format!("transcript read panicked: {e}")),
            }
        }
    };
    Json(page).into_response()
}

/// `GET /runs/{run_id}/nodes/{node_id}/transcript/tools/{index}`: the input
/// and the output of the tool call at `index`, read on demand.
pub async fn tool_detail(
    state: &Runs,
    run_id: &str,
    node_id: &str,
    iter: i64,
    index: usize,
) -> Response {
    let (harness, path) = match source(state, run_id, node_id, iter).await {
        Ok(TranscriptSource::File { harness, path }) => (harness, path),
        Ok(_) => return not_found("no transcript for this node iteration"),
        Err(response) => return response,
    };
    let read = tokio::task::spawn_blocking(move || {
        TranscriptReader::shared().tool_detail(&harness, &path, index)
    })
    .await;
    match read {
        Ok(Ok(Some(detail))) => Json(detail).into_response(),
        Ok(Ok(None)) => not_found("no tool call at this index"),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            not_found("no transcript for this node iteration")
        }
        Ok(Err(e)) => internal(format!("cannot read the transcript: {e}")),
        Err(e) => internal(format!("transcript read panicked: {e}")),
    }
}

async fn source(
    state: &Runs,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Result<TranscriptSource, Response> {
    let (events, run_state) = load_projected(state, run_id).await.map_err(|r| *r)?;
    if !run_state.nodes.contains_key(node_id)
        && !run_state.node_defs.iter().any(|nd| nd.id == node_id)
    {
        return Err(not_found("node not found in run"));
    }
    let roots = match sandbox_home_roots(state) {
        Ok((home_root, sandbox_root)) => StoreRoots {
            home_root,
            sandbox_root,
        },
        Err(e) => return Err(internal(format!("{e:#}"))),
    };
    let repo_root = effective_repo_root(state, &run_state);
    Ok(locate(
        &events, &run_state, &repo_root, node_id, iter, &roots,
    ))
}

fn not_found(reason: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": reason })),
    )
        .into_response()
}

fn internal(reason: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": reason })),
    )
        .into_response()
}
