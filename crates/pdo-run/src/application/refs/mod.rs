//! A Run's refs (#749, ADR-0067 §1) — its fork point, its tip, each delivery's
//! `before`/`after`, a live node's branch, its working tree — and the full
//! content of one file at one of them, for the Review page.

use std::path::PathBuf;

use axum::extract::Json;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use pdo_workspace::adapters::git::revisions;
use pdo_workspace::adapters::git::worktrees::{is_git_worktree, worktree_dir_for_run};
use serde::Deserialize;

use crate::application::events::load_projected;
use crate::application::paths::effective_repo_root;
use crate::domain::event::Event;
use crate::domain::run_refs;
use crate::domain::state::RunState;
use crate::Runs;

pub mod source_drift;

/// The Run's shared worktree, when it still exists on disk (#835). `None` once
/// the Run is finished and cleaned up, or archived: the `worktree` ref is gone.
pub fn run_worktree_dir(state: &Runs, run_state: &RunState, run_id: &str) -> Option<PathBuf> {
    let dir = worktree_dir_for_run(&effective_repo_root(state, run_state), run_id);
    is_git_worktree(&dir).then_some(dir)
}

/// The id a `from`/`to`/`ref` query defaults to when absent: the working tree
/// while it exists (#835), the tip otherwise — the same rule as `run_refs::collect`.
pub fn default_to_id(worktree: Option<&PathBuf>) -> &'static str {
    if worktree.is_some() {
        run_refs::WORKTREE_ID
    } else {
        run_refs::TIP_ID
    }
}

/// What a `from`/`to`/`ref` query value resolved to (#835).
pub enum RefTarget {
    /// A git ref (branch, SHA) to hand to `git`.
    Git(String),
    /// The Run's working tree — read from the worktree dir / its snapshot tree.
    Worktree,
}

#[derive(Deserialize)]
pub struct FileAtRefQuery {
    pub path: String,
    /// Defaults to the Run's working tree while it exists (#835), else to the
    /// Run's tip (`pdo/run-<id>`).
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

impl Runs {
    /// `GET /runs/<id>/file?path=<repo-relative>&ref=<ref>` (#748): the full content
    /// of one file at one Run ref, for the Review page's context expansion. Text is
    /// served as `text/plain`, anything else as `application/octet-stream`; a path
    /// missing at that ref is a 404.
    pub async fn file_at_ref(&self, run_id: &str, q: FileAtRefQuery) -> Response {
        let state = self;
        let run_id = run_id.to_string();
        let (events, run_state) = match load_projected(state, &run_id).await {
            Ok(t) => t,
            Err(resp) => return *resp,
        };
        if !revisions::is_safe_path(&q.path) {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid path: {:?}", q.path),
            )
                .into_response();
        }
        // #749: stable Run ref ids resolve like on the structured diff endpoint.
        let worktree = run_worktree_dir(state, &run_state, &run_id);
        let target = match resolve_ref_param(
            &run_id,
            &run_state,
            &events,
            q.git_ref.as_deref(),
            default_to_id(worktree.as_ref()),
        ) {
            Ok(r) => r,
            Err(resp) => return *resp,
        };
        let repo = effective_repo_root(state, &run_state);
        let content = match target {
            RefTarget::Git(git_ref) => revisions::file_at_ref(&repo, &git_ref, &q.path),
            // #835: the working tree's file is the file on disk (untracked included).
            RefTarget::Worktree => {
                let Some(dir) = worktree else {
                    return (StatusCode::NOT_FOUND, "run worktree not found").into_response();
                };
                match std::fs::read(dir.join(&q.path)) {
                    Ok(bytes) => Ok(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return (StatusCode::NOT_FOUND, "file not found at ref").into_response();
                    }
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
                    }
                }
            }
        };
        match content {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    text,
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/octet-stream")],
                    e.into_bytes(),
                )
                    .into_response(),
            },
            Err(revisions::GitError::Failed { stderr })
                if stderr.contains("does not exist")
                    || stderr.contains("exists on disk, but not in")
                    || stderr.contains("unknown revision")
                    || stderr.contains("not a git repository")
                    || stderr.contains("bad revision")
                    || stderr.contains("nvalid object name") =>
            {
                (StatusCode::NOT_FOUND, "file not found at ref").into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

/// Resolve one `from`/`to`/`ref` query value (#749): absent ⇒ `default_id`; a
/// stable Run ref id ⇒ its git ref (404 when the Run knows no such ref) or the
/// working tree (#835); anything else ⇒ a raw git ref, kept for compatibility
/// once it passes the injection guard.
pub fn resolve_ref_param(
    run_id: &str,
    run_state: &RunState,
    events: &[Event],
    value: Option<&str>,
    default_id: &str,
) -> Result<RefTarget, Box<Response>> {
    let value = value.unwrap_or(default_id);
    match run_refs::resolve_id(run_id, run_state, events, value) {
        run_refs::Resolved::Git(r) => Ok(RefTarget::Git(r)),
        run_refs::Resolved::Worktree => Ok(RefTarget::Worktree),
        run_refs::Resolved::Unknown => Err(Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("unknown run ref: {value}") })),
            )
                .into_response(),
        )),
        run_refs::Resolved::NotAnId => {
            if !revisions::is_safe_ref(value) {
                return Err(Box::new(
                    (StatusCode::BAD_REQUEST, format!("invalid ref: {value:?}")).into_response(),
                ));
            }
            Ok(RefTarget::Git(value.to_string()))
        }
    }
}

impl Runs {
    /// `GET /runs/<id>/refs` (#749, ADR-0067 §1): the Run's refs — fork point, Run
    /// tip, every node delivery's `before`/`after` from the event log (labelled by
    /// node name and `iter`), the live sub-worktree branch of a running isolated
    /// node, and the Run's working tree while it exists (#835, the default
    /// destination) — plus the ready-made delivery pairs. Labels are built here so
    /// the UI and the CLI agree. SHAs are resolved in the Run's effective repository.
    pub async fn run_refs(&self, run_id: &str) -> Response {
        let state = self;
        let run_id = run_id.to_string();
        let (events, run_state) = match load_projected(state, &run_id).await {
            Ok(t) => t,
            Err(resp) => return *resp,
        };
        let repo = effective_repo_root(state, &run_state);
        let sha_of = |r: &str| revisions::rev_parse(&repo, r);
        let snapshot = run_worktree_dir(state, &run_state, &run_id)
            .map(|dir| revisions::snapshot_worktree(&dir).ok());
        Json(run_refs::collect(
            &run_id,
            &run_state,
            &events,
            &sha_of,
            snapshot.as_ref().map(Option::as_deref),
        ))
        .into_response()
    }
}
