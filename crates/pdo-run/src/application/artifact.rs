//! `GET /runs/{run_id}/artifact`: one artifact a node wrote, served from the
//! Run's live worktree or, once archived, from the durable store (ADR-0020),
//! never outside its artifacts directory.

use std::path::Path;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::application::events::load_projected;
use crate::application::paths::archived_or_live_artifacts_dir;
use crate::Runs;

#[derive(Deserialize)]
pub struct ArtifactQuery {
    pub path: String,
}

impl Runs {
    /// `GET /runs/{run_id}/artifact?path=…`: one artifact of the Run, from its
    /// live worktree or, once archived, from the durable store.
    pub async fn artifact(&self, run_id: &str, query: ArtifactQuery) -> Response {
    let state = self;
    let (_, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    // Serve from the durable store for an archived run (worktree gone), from the
    // live worktree otherwise. The `.canonicalize()` guard below holds against
    // whichever base is resolved.
    let artifacts_dir = archived_or_live_artifacts_dir(state, &run_state, run_id);

    let requested = Path::new(&query.path);
    let resolved = match artifacts_dir.join(requested).canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response();
        }
    };

    let canonical_artifacts = match artifacts_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "artifacts directory not found").into_response();
        }
    };

    if !resolved.starts_with(&canonical_artifacts) {
        return (StatusCode::BAD_REQUEST, "path traversal not allowed").into_response();
    }

    let mime = match resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        // An `.html` artifact DELIBERATELY falls through to `text/markdown`. Do NOT
        // add `Some("html") => "text/html"` (ADR-0028): serving agent-authored HTML as
        // a top-level document at the daemon origin — no auth, no CSP — would let it
        // execute arbitrary script. It is only ever rendered in a sandboxed iframe.
        _ => "text/markdown",
    };

    // #796: an artifact URL is stable across a same-iter re-spawn (same run,
    // node, iter and path) while its bytes change under it. Without a validator
    // the browser's heuristic cache may resurface the previous session's report
    // or screenshot; `no-store` makes every reload show the current bytes.
    let headers = [
        (header::CONTENT_TYPE, mime),
        (header::CACHE_CONTROL, "no-store"),
    ];
    if mime.starts_with("image/") {
        match std::fs::read(&resolved) {
            Ok(bytes) => (StatusCode::OK, headers, bytes).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    } else {
        match std::fs::read_to_string(&resolved) {
            Ok(content) => (StatusCode::OK, headers, content).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    }
}
}
