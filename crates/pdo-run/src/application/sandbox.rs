//! The Run's side of its sandbox: what the run context hands the `sandbox`
//! context (crates/pdo-sandbox) — a Run's frozen choice and paths, the
//! per-daemon overrides — and the `docker exec` of a session into the Run's
//! container.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pdo_sandbox::adapters::host::{host_gid, host_uid, pdo_bin_path, roots_under};
use pdo_sandbox::adapters::{env as sandbox_env, local_runtime, staging};
use pdo_sandbox::application::context::{assemble_context, RunSandbox, SandboxContext};
use pdo_sandbox::application::prepare::SandboxRuntime;
use pdo_sandbox::domain::frozen::FrozenSandbox;
use pdo_session::domain::launch::{ContainerExec, SandboxWrap};
use tracing::warn;

use crate::domain::state::RunState;
use crate::Runs;

/// Where a Run's harness transcripts are read from: the Run's seam onto its staging.
pub use pdo_sandbox::adapters::staging::stores::{transcripts_root, HarnessStores};

/// A Run's sandbox, ready to be prepared: its context and the runtime that
/// prepares it. [`Self::ensure_ready`] blocks — call it from `spawn_blocking`.
pub struct RunSandboxPrep {
    pub ctx: SandboxContext,
    runtime: SandboxRuntime,
}

impl RunSandboxPrep {
    /// See [`SandboxRuntime::ensure_ready`].
    pub fn ensure_ready(&self) -> Result<()> {
        self.runtime.ensure_ready(&self.ctx)
    }
}

/// The sandbox runtime of this daemon: its docker, the local staged homes and
/// Dockerfiles.
pub fn runtime(state: &Runs) -> SandboxRuntime {
    local_runtime(state.docker_cmd_override.as_deref())
}

/// `(home_root, sandbox_root)` honouring the per-daemon override (#407 test seam):
/// `Some(dir)` → `(dir, dir/.pdo/sandbox)`; else the real `$HOME`.
pub fn sandbox_home_roots(state: &Runs) -> Result<(PathBuf, PathBuf)> {
    if let Some(home) = &state.sandbox_home_override {
        return Ok(roots_under(home));
    }
    pdo_sandbox::adapters::host::default_roots_from_env()
        .context("HOME is not set; cannot resolve the sandbox staging root")
}

/// The sandbox half of a Run's projection.
fn frozen(run_state: &RunState) -> FrozenSandbox {
    FrozenSandbox {
        run_id: run_state.run_id.clone(),
        entries: run_state.sandbox_entries.clone(),
        entries_raw_error: run_state.sandbox_entries_raw_error.clone(),
        env: run_state.sandbox_env.clone(),
        env_raw_error: run_state.sandbox_env_raw_error.clone(),
        image: run_state.sandbox_image.clone(),
        image_raw_error: run_state.sandbox_image_raw_error.clone(),
    }
}

/// Resolve a Run's sandbox from the daemon state + its projection. The one edge that
/// reads `Runs` / the environment for it.
///
/// Fails (loud) when `$HOME` is unset, the current exe path can't be resolved, or the
/// Run's **frozen** staging-profile selection cannot be resolved (#432) — a sandboxed
/// Run must never fall back to a half-configured container, or to a *different* home
/// content, silently. The one caller that used to swallow this error (boot recovery)
/// turns it into an explicit `RunFailed`.
pub async fn context_from_state(state: &Runs, run_state: &RunState) -> Result<RunSandboxPrep> {
    let repo_root = crate::application::paths::effective_repo_root(state, run_state);
    let run_worktree = pdo_workspace::adapters::git::worktrees::worktree_dir_for_run(
        &repo_root,
        &run_state.run_id,
    );
    let (home_root, sandbox_root) = sandbox_home_roots(state)?;
    let pdo_bin = pdo_bin_path()?;
    // ADR-0047: the `.git` of every WRITABLE secondary — outside `repo_root`, so the
    // single repo mount does not cover it. Read-only secondaries get nothing.
    let writable_secondary_gitdirs = run_state
        .target_repos
        .iter()
        .filter(|pin| !pin.read_only)
        .map(|pin| Path::new(&pin.repo).join(".git"))
        .collect();
    let run = RunSandbox {
        mode: run_state.sandbox.clone(),
        frozen: frozen(run_state),
        repo_root,
        run_worktree,
        daemon_port: state.port,
        home_root,
        sandbox_root,
        uid: host_uid(),
        gid: host_gid(),
        pdo_bin,
        writable_secondary_gitdirs,
        env_image_source: sandbox_env::env_image_source(),
        env_dockerfile: sandbox_env::env_dockerfile_path(),
    };
    let ctx = assemble_context(state.sandbox_profiles.as_ref(), run).await?;
    Ok(RunSandboxPrep {
        ctx,
        runtime: runtime(state),
    })
}

/// Merge a Run's staged transcripts back to `~/.claude/projects/` at its terminal
/// transition (#408), so cost + stale-detection see them at the standard encoded
/// dirname once the staging is eventually purged. No-op for `off`.
///
/// Never fails / slows the caller: it re-projects the Run, resolves the roots, then
/// fires the filesystem walk on a **detached** `spawn_blocking` — the terminal
/// transition must not depend on this merge (ADR-0023). Idempotent, so a second
/// terminal event or the later `cleanup_run` merge is safe.
pub async fn merge_back_best_effort(state: &Runs, run_id: &str) {
    let Some((_, run_state)) = crate::application::events::reload_run_state(state, run_id).await
    else {
        return;
    };
    if run_state.sandbox.is_off() {
        return;
    }
    let (home_root, sandbox_root) = match sandbox_home_roots(state) {
        Ok(r) => r,
        Err(e) => {
            warn!("merge_back: cannot resolve roots for run {run_id}: {e:#}");
            return;
        }
    };
    let rid = run_id.to_string();
    // Fire-and-forget: capture owned values, do NOT `.await` (that would recouple the
    // terminal transition to the merge's latency + JoinError).
    tokio::task::spawn_blocking(move || {
        if let Err(e) = staging::merge_back(&home_root, &sandbox_root, &rid) {
            warn!("sandbox merge_back for run {rid} failed (best-effort): {e:#}");
        }
    });
}

/// Fill `harness`'s staging set into a sandboxed Run's staging **once per Run**, and
/// return the env the session's `docker exec` must carry (#708, ADR-0063 §3). The
/// entry point for the infra sessions (manager, reattach); `spawn_node`
/// calls [`staging::fill_staging_set`] with the roots it already holds. A no-op (empty
/// env) for `off`, and for a harness with no set. Hard error when the set cannot be
/// copied: a session on an unfillable home would hang on a dialog.
pub fn stage_harness_set(
    state: &Runs,
    run_state: &RunState,
    harness: &str,
    trusted_root: &Path,
) -> Result<Vec<(String, String)>> {
    if run_state.sandbox.is_off() {
        return Ok(Vec::new());
    }
    let (home_root, sandbox_root) = sandbox_home_roots(state)?;
    let fill = staging::fill_staging_set(
        &home_root,
        &sandbox_root,
        &run_state.run_id,
        harness,
        Some(trusted_root),
    )
    .with_context(|| {
        format!(
            "failed to stage the `{harness}` set for run {}",
            run_state.run_id
        )
    })?;
    Ok(fill.env())
}

/// Best-effort targeted kill of one session inside the Run's container (#407 D8).
/// No-op for `off`.
pub fn kill_session_best_effort(docker_bin: &str, sandboxed: bool, run_id: &str, marker: &str) {
    local_runtime(Some(docker_bin)).kill_session_best_effort(
        sandboxed,
        run_id,
        marker,
        host_uid(),
        host_gid(),
    );
}

/// Merge back, destroy the container, purge the staging (`cleanup_run`). Best-effort.
pub fn cleanup(docker_bin: &str, home_root: &Path, sandbox_root: &Path, run_id: &str) {
    local_runtime(Some(docker_bin)).cleanup(home_root, sandbox_root, run_id);
}
/// One session's way into its Run's container.
pub struct SandboxExec {
    /// The `docker` binary to invoke (per-daemon override → `"docker"`).
    docker_bin: String,
    run_id: String,
    /// The session marker: **MUST** equal the tmux session name the kill path
    /// uses (`PDO_SBX_SESSION`), or the targeted `/proc` kill misses its tree.
    marker: String,
    /// The session's working dir → `docker exec -w <workdir>` (the load-bearing
    /// cwd inside the container).
    workdir: PathBuf,
    /// The resolved harness's staging-set env (#708), forwarded on the exec.
    set_env: Vec<(String, String)>,
}

impl SandboxExec {
    pub fn new(
        docker_override: Option<&str>,
        run_id: &str,
        marker: &str,
        workdir: &Path,
        set_env: &[(String, String)],
    ) -> Self {
        Self {
            docker_bin: docker_override.unwrap_or("docker").to_string(),
            run_id: run_id.to_string(),
            marker: marker.to_string(),
            workdir: workdir.to_path_buf(),
            set_env: set_env.to_vec(),
        }
    }

    /// The wrap the session launch splices its tail into.
    pub fn wrap(&self) -> SandboxWrap<'_> {
        SandboxWrap {
            exec: self,
            set_env: &self.set_env,
        }
    }
}

impl ContainerExec for SandboxExec {
    fn exec_argv(&self, names: &[String]) -> Vec<String> {
        let mut argv = vec![self.docker_bin.clone()];
        argv.extend(pdo_sandbox::domain::container::exec_prefix_with_env(
            &self.run_id,
            pdo_sandbox::adapters::host::host_uid(),
            pdo_sandbox::adapters::host::host_gid(),
            &self.workdir,
            &self.marker,
            names,
        ));
        argv
    }
}
