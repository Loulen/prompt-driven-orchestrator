//! Where a Run's files are: its repository, its run-scoped pipeline and
//! prompts, its durable copy once archived.

use std::path::{Path, PathBuf};

use crate::domain::state;
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// Resolve the repository a Run works in, for a **projected** `RunState`.
///
/// The fallback to the daemon's `repo_root` is deliberate and permanent (ADR-0033):
/// `run_started` events written before the create boundary was hardened legitimately
/// carry `target_repo: null`, and every caller here is a READ. Making this fallible
/// or removing the fallback would break re-reading the whole history.
///
/// Do NOT "restore symmetry" with the create boundary: the asymmetry IS the design.
/// `target_repo` is required where there is a caller to answer 400 to, and resolved
/// where there is only a past record to interpret.
pub fn effective_repo_root(state: &Runs, run_state: &state::RunState) -> PathBuf {
    run_state
        .target_repo
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| state.repo_root.clone())
}

pub fn run_scoped_pipeline_path(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("pipeline.yaml")
}

pub fn run_scoped_prompts_dir(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("pipeline.prompts")
}

/// A run's artifacts dir: the durable global store when the run is `Archived` (its
/// repo-local worktree is gone), the live worktree otherwise.
pub fn archived_or_live_artifacts_dir(
    state: &Runs,
    run_state: &state::RunState,
    run_id: &str,
) -> PathBuf {
    if run_state.status == state::RunStatus::Archived {
        if let Some(durable) = durable_run_dir(run_id) {
            return durable.join("artifacts");
        }
    }
    let repo_root = effective_repo_root(state, run_state);
    worktree_dir_for_run(&repo_root, run_id)
        .join(".pdo")
        .join("artifacts")
}

/// A run's `pipeline.yaml`: durable store when `Archived`, else the live run-scoped
/// path.
pub fn archived_or_live_pipeline_yaml(
    state: &Runs,
    run_state: &state::RunState,
    run_id: &str,
) -> PathBuf {
    if run_state.status == state::RunStatus::Archived {
        if let Some(durable) = durable_run_dir(run_id) {
            return durable.join("pipeline.yaml");
        }
    }
    let repo_root = effective_repo_root(state, run_state);
    run_scoped_pipeline_path(&repo_root, run_id)
}

/// A run's prompts dir: durable store when `Archived`, else the live run-scoped path.
pub fn archived_or_live_prompts_dir(
    state: &Runs,
    run_state: &state::RunState,
    run_id: &str,
) -> PathBuf {
    if run_state.status == state::RunStatus::Archived {
        if let Some(durable) = durable_run_dir(run_id) {
            return durable.join("pipeline.prompts");
        }
    }
    let repo_root = effective_repo_root(state, run_state);
    run_scoped_prompts_dir(&repo_root, run_id)
}

/// The instance registry of the pipeline context. A test binary of the
/// workspace (debug build, run from cargo's `deps/` directory: this crate's
/// tests, the legacy crate's, the daemon's integration tests) keeps it inside
/// its temporary repository, as the pipeline context's own registry does (see
/// [`pdo_pipeline::adapters::registry::instance_pipeline_dir`]): a test never
/// reads the real `~/.pdo/pipelines`.
pub fn instance_pipeline_dir(repo_root: &Path) -> Option<PathBuf> {
    if cfg!(test) || cargo_test_binary() {
        return Some(repo_root.join(".pdo").join("pipelines"));
    }
    pdo_pipeline::adapters::registry::instance_pipeline_dir(repo_root)
}

fn cargo_test_binary() -> bool {
    cfg!(debug_assertions)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .and_then(|dir| dir.file_name())
                    .map(|name| name == "deps")
            })
            .unwrap_or(false)
}

pub fn resolve_pipeline_path(repo_root: &std::path::Path, pipeline_name: &str) -> PathBuf {
    instance_pipeline_dir(repo_root)
        .unwrap_or_else(|| repo_root.join(".pdo").join("pipelines"))
        .join(format!("{pipeline_name}.yaml"))
}

#[allow(dead_code)]
pub fn repo_pipeline_path(repo_root: &std::path::Path, pipeline_name: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("pipelines")
        .join(format!("{pipeline_name}.yaml"))
}

pub fn resolve_run_pipeline_path(
    repo_root: &std::path::Path,
    run_id: &str,
    pipeline_name: &str,
) -> PathBuf {
    let run_scoped = run_scoped_pipeline_path(repo_root, run_id);
    if run_scoped.exists() {
        run_scoped
    } else {
        resolve_pipeline_path(repo_root, pipeline_name)
    }
}

pub fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Durable, repo-independent home for a run's preserved outputs (ADR-0020). Under
/// `$HOME`, so it survives `cleanup_run`'s `remove_dir_all(<repo>/.pdo/runs/<id>)`.
/// `None` when `$HOME` is unset — callers skip, never fail teardown.
pub fn durable_run_dir(run_id: &str) -> Option<PathBuf> {
    dirs_next_home().map(|home| home.join(".pdo").join("runs").join(run_id))
}
