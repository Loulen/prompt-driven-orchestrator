//! What a new Run works in and from: its repository (required at the create
//! boundary, ADR-0033), the branch it is cut from, its frozen secondary pins
//! (ADR-0042), and the pipeline it runs, loaded and checked before anything
//! exists on disk.

use std::path::PathBuf;

use axum::http::StatusCode;
use pdo_pipeline::domain::pipeline::{self, PipelineDef};
use pdo_workspace::adapters::git::repository::validate_source_branch;
use tracing::warn;

use super::naming::validate_run_input;
use super::repos::{required_target_repo, resolve_secondary_pins};
use super::request::CreateRunRequest;
use crate::application::paths::{effective_repo_root, resolve_pipeline_path};
use crate::domain::state::{RepoPin, RunState};
use crate::Runs;

/// A refusal of the create path: the status and the JSON body it answers.
pub type Refusal = (StatusCode, serde_json::Value);

pub(super) fn bad_request(message: impl Into<String>) -> Refusal {
    (
        StatusCode::BAD_REQUEST,
        serde_json::json!({ "error": message.into() }),
    )
}

/// The repositories of a new Run, resolved.
pub(super) struct Target {
    pub run_repo_root: PathBuf,
    /// The primary's base branch as the request named it, if it did.
    pub source_branch: Option<String>,
    /// The ref the Run branch is cut from: the source branch, else `HEAD`.
    pub source_ref: String,
    pub secondary_pins: Vec<RepoPin>,
}

/// Resolve the Run's repositories. `parent` is the projected state of the Run
/// whose node session created this one (ADR-0064).
pub(super) fn resolve(
    state: &Runs,
    req: &CreateRunRequest,
    parent: Option<&RunState>,
) -> Result<Target, Refusal> {
    // The target repo is REQUIRED at the creation boundary (ADR-0033): the daemon's
    // own `repo_root` stays its storage root but is never an implicit Run target, or
    // a Run mutates code in a repository nobody named. The read side keeps its
    // fallback for historical runs (`effective_repo_root`).
    //
    // Normalise the multi-repo shape down to the scalar here, so the ENTIRE
    // downstream primary path (worktree, merge-back, cost) stays unchanged and the
    // array only ever ADDS secondaries.
    //
    // One exception, itself mechanical (ADR-0064): a Run created from a node
    // session defaults to the PARENT's project when the request names none — an
    // orchestrated child is an ordinary run whose common case inherits the
    // orchestrator's repo. An explicit `target_repo`/`target_repos` always wins.
    let effective_target_repo: Option<String> = req
        .target_repo
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            req.target_repos
                .first()
                .map(|r| r.repo.clone())
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| parent.map(|p| effective_repo_root(state, p).to_string_lossy().into_owned()));
    let run_repo_root = required_target_repo(effective_target_repo.as_deref()).map_err(bad_request)?;

    // The primary's base branch is the scalar `source_branch` when present, else
    // `target_repos[0].base_branch`: row 0 of the multi-repo modal carries the
    // primary's branch in the same field the secondaries use.
    let source_branch: Option<String> = req
        .source_branch
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            req.target_repos
                .first()
                .and_then(|r| r.base_branch.clone())
                .filter(|s| !s.trim().is_empty())
        });
    let source_ref = match &source_branch {
        Some(branch) => {
            validate_source_branch(&run_repo_root, branch).map_err(bad_request)?;
            branch.clone()
        }
        None => "HEAD".to_string(),
    };

    // Resolve the secondary repos into frozen pins (ADR-0042). Each is validated
    // (absolute + git) and its base ref resolved to a single commit NOW — no fetch,
    // so the base is the operator's LOCAL ref. Must resolve BEFORE
    // `append_event`/`create_worktree`, or "a bad repo → no Run" stops holding.
    let secondary_pins =
        resolve_secondary_pins(&run_repo_root, &req.target_repos).map_err(bad_request)?;

    Ok(Target {
        run_repo_root,
        source_branch,
        source_ref,
        secondary_pins,
    })
}

/// The pipeline a new Run runs, read from the instance registry.
pub(super) struct Launchable {
    pub pipeline: PipelineDef,
    /// The template file it was read from, copied into the Run's directory.
    pub pipeline_path: PathBuf,
}

/// Load the pipeline and refuse one a Run cannot run: unreadable or
/// unparsable, a dangling edge, an empty script, a required prompt missing.
pub(super) fn load_pipeline(state: &Runs, req: &CreateRunRequest) -> Result<Launchable, Refusal> {
    let (yaml, pipeline_path) = if let Some(ref pipeline_id) = req.pipeline_id {
        let path = resolve_pipeline_path(&state.repo_root, pipeline_id);
        match std::fs::read_to_string(&path) {
            Ok(yaml) => (yaml, path),
            Err(_) => return Err(bad_request(format!("pipeline not found: {pipeline_id}"))),
        }
    } else {
        let path = resolve_pipeline_path(&state.repo_root, &req.pipeline);
        match std::fs::read_to_string(&path) {
            Ok(y) => (y, path),
            Err(e) => return Err(bad_request(format!("cannot read pipeline: {e}"))),
        }
    };

    let parse_result = pipeline::parse_pipeline(&yaml)
        .map_err(|e| bad_request(format!("pipeline parse error: {e}")))?;
    for diag in &parse_result.diagnostics {
        warn!("pipeline {}: {}", req.pipeline, diag.message);
    }
    let pipeline = parse_result.pipeline;

    // Dangling edge references are info-only warnings at EDIT time (ADR-0001) but
    // hard refusals at launch: a run started over a dangling port is guaranteed to
    // stall silently mid-run.
    let dangling = pipeline::dangling_edge_references(&pipeline);
    if !dangling.is_empty() {
        return Err(bad_request(format!(
            "cannot launch run: pipeline has dangling edge reference(s): {}",
            dangling.join("; ")
        )));
    }

    // A `script` node runs `bash <body>`, and an empty body exits 0 — a silent
    // no-op masquerading as success. Fail loud at launch, before any run exists.
    let empty_scripts: Vec<String> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Script)
        .filter(|n| {
            let path = pipeline::canonical_prompt_path(&pipeline_path, &n.id);
            std::fs::read_to_string(&path)
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|n| n.id.clone())
        .collect();
    if !empty_scripts.is_empty() {
        return Err(bad_request(format!(
            "cannot launch run: script node(s) have an empty body: {}",
            empty_scripts.join(", ")
        )));
    }

    validate_run_input(pipeline.prompt_required, &req.input).map_err(bad_request)?;
    Ok(Launchable {
        pipeline,
        pipeline_path,
    })
}
