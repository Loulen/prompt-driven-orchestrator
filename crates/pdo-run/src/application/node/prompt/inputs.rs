//! Where a node's inputs and outputs are on the blackboard, the files its
//! Run was launched with, and the partial output an interrupted attempt left.

use std::path::{Path, PathBuf};

use pdo_pipeline::domain::pipeline::{NodeDef, PortType};

use super::*;

pub fn discover_input_images(artifacts_dir: &Path) -> Vec<String> {
    let input_dir = artifacts_dir.join("_input");
    let entries = match std::fs::read_dir(&input_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut images = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        if ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                images.push(name.to_string());
            }
        }
    }
    images.sort();
    images
}

/// The NON-image attachments of `_input/` (#779): every regular file that is
/// neither the prompt (`output.md`) nor an image (those are
/// [`discover_input_images`]). Sorted for a stable preamble.
pub fn discover_input_files(artifacts_dir: &Path) -> Vec<String> {
    let input_dir = artifacts_dir.join("_input");
    let entries = match std::fs::read_dir(&input_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "output.md" {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        if ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        files.push(name.to_string());
    }
    files.sort();
    files
}

/// `Ok(false)` when the file is absent — the expected prompt-optional case, not
/// an error. `Err` only on a genuine I/O failure, so the caller surfaces it
/// instead of silently reporting "no prompt".
pub fn read_start_prompt_present(artifacts_dir: &Path) -> std::io::Result<bool> {
    match std::fs::read_to_string(crate::domain::blackboard::input_path(artifacts_dir)) {
        Ok(text) => Ok(!text.trim().is_empty()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn resolve_input_paths(ctx: &AugmentContext<'_>) -> Vec<InputResolution> {
    // Project over the SINGLE edge-walk: the iteration decision lives in
    // `input_resolution::resolve_consumer_inputs`. The preamble and script-env are
    // pure projections over its result — never a second, independent edge-walk.
    let mut inputs: Vec<InputResolution> =
        crate::domain::input_resolution::resolve_consumer_inputs(
            ctx.pipeline,
            ctx.artifacts_dir,
            &ctx.node.id,
            ctx.iter,
            &ctx.source_iters,
            &ctx.repeated_iters,
        )
        .into_iter()
        .map(|r| InputResolution {
            // ADR-0086: an upstream that has produced nothing in this Run (a node
            // launched ahead of it) gives no path — never one nothing wrote.
            paths: if !r.from_start && ctx.absent_sources.contains(&r.source_node) {
                Vec::new()
            } else {
                r.paths
            },
            port_name: r.port,
            source_node: r.source_node,
            repeated: r.repeated,
            from_start: r.from_start,
        })
        .collect();

    if inputs.is_empty() && ctx.node.inputs.iter().any(|p| p.name == "task") {
        inputs.push(InputResolution {
            port_name: "task".into(),
            source_node: String::new(),
            paths: vec![crate::domain::blackboard::input_path(ctx.artifacts_dir)],
            repeated: false,
            from_start: true,
        });
    }

    inputs
}

/// The on-disk path a single output port declares, for one iteration. The single
/// source of truth for output-port path math (shared by [`resolve_output_paths`]
/// and [`surviving_partial_outputs`]).
fn output_port_path(
    node_id: &str,
    artifacts_dir: &Path,
    iter: i64,
    port: &pdo_pipeline::domain::pipeline::Port,
) -> PathBuf {
    match port.port_type {
        PortType::Image | PortType::ImageList => {
            crate::domain::blackboard::port_dir(artifacts_dir, node_id, iter, &port.name)
        }
        PortType::Markdown => {
            crate::domain::blackboard::artifact_path(artifacts_dir, node_id, iter, &port.name)
        }
        // An html port's declared path is its `output.html` file (parallel to
        // markdown's `output.md`), not the port dir.
        PortType::Html => {
            crate::domain::blackboard::artifact_path_html(artifacts_dir, node_id, iter, &port.name)
        }
    }
}

pub fn resolve_output_paths(ctx: &AugmentContext<'_>) -> Vec<OutputDeclaration> {
    ctx.node
        .outputs
        .iter()
        .map(|port| OutputDeclaration {
            port_name: port.name.clone(),
            path: output_port_path(&ctx.node.id, ctx.artifacts_dir, ctx.iter, port),
            port_type: port.port_type,
            intermediate: port.intermediate,
        })
        .collect()
}

/// The declared output paths that ALREADY hold content on disk for `iter` — what
/// an interrupted attempt left behind (ADR-0049). A same-iter re-spawn never
/// wipes it, so the fresh agent must be shown it and told to build on it. Empty
/// on a first spawn.
///
/// #796: a path older than `run_started_at` is NOT partial output of this run —
/// it came in through git (a child run created from a branch that carries a
/// previous run's `.pdo/artifacts/`, same pipeline hence same node ids and
/// paths). Handing it to the agent as "your interrupted attempt" misleads it into
/// building on another ticket's report; such paths are left out. `None` (run
/// start unknown) keeps the pre-#796 behaviour.
///
/// Does I/O, so the daemon computes it and feeds
/// [`AugmentContext::partial_outputs`]; `build_preamble` itself stays pure.
pub fn surviving_partial_outputs(
    node: &NodeDef,
    artifacts_dir: &Path,
    iter: i64,
    run_started_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<PathBuf> {
    // #796: a file git tracks UNMODIFIED came in with the worktree checkout —
    // another run's report inherited through the source branch, never an
    // interrupted attempt of THIS run. The mtime gate alone misses it: the
    // checkout happens a few ms AFTER the run record starts. A tracked file the
    // working copy has modified is this run's work (an interrupted attempt
    // overwrote the inherited report) and is handed over like any survivor.
    let tracked = crate::application::node::io::git_tracked_paths(artifacts_dir, &node.id, iter);
    // An *Output intermédiaire* is no partial output: it is meant to be
    // rewritten as the work goes, and the preamble already says where it is.
    node.outputs
        .iter()
        .filter(|port| !port.intermediate)
        .map(|port| output_port_path(&node.id, artifacts_dir, iter, port))
        .filter(|path| path_holds_content(path))
        .filter(|path| !crate::application::node::io::is_inherited(path, run_started_at, &tracked))
        .filter(|path| !dir_only_tracked(path, &tracked))
        .collect()
}

/// An image/html port DIR whose every entry git tracks is inherited as a whole
/// (`git ls-files` names files, never directories). A dir with at least one
/// untracked entry keeps some of this run's work and is handed over.
pub(super) fn dir_only_tracked(path: &Path, tracked: &std::collections::HashSet<PathBuf>) -> bool {
    if !path.is_dir() || tracked.is_empty() {
        return false;
    }
    std::fs::read_dir(path)
        .is_ok_and(|entries| entries.flatten().all(|e| tracked.contains(&e.path())))
}

/// Does `path` hold real output? A file with non-whitespace bytes, or a directory
/// with at least one entry (an image/html port dir). A missing path, or an empty
/// file/dir, is "no partial output" — the ordinary first-spawn state.
pub(super) fn path_holds_content(path: &Path) -> bool {
    if path.is_dir() {
        return std::fs::read_dir(path).is_ok_and(|mut e| e.next().is_some());
    }
    std::fs::read_to_string(path).is_ok_and(|s| !s.trim().is_empty())
}
