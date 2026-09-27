//! The Run's side of its sandbox: what the legacy daemon hands the `sandbox`
//! context (crates/pdo-sandbox) — a Run's frozen choice and paths, the per-daemon
//! overrides — and the port it implements for it (who still names a profile).
//!
//! Run-context code: it reads `AppState` and the projected `RunState`, which the
//! sandbox context never sees. It moves out with the Run (#923).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_sandbox::adapters::host::{host_gid, host_uid, pdo_bin_path, roots_under, EnvHome};
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_sandbox::adapters::{env as sandbox_env, local_runtime, staging};
use pdo_sandbox::application::context::{assemble_context, RunSandbox, SandboxContext};
use pdo_sandbox::application::ports::{
    BoxFuture, ProfileReferents, Referents, RunReferent, TriggerReferent,
};
use pdo_sandbox::application::prepare::SandboxRuntime;
use pdo_sandbox::application::profiles::SandboxProfiles;
use pdo_sandbox::domain::frozen::FrozenSandbox;
use pdo_sandbox::domain::mode::SandboxMode;
use tracing::{error, warn};

use crate::event_log::RunState;

use crate::{instance_config, trigger_store, AppState};
/// Where a Run's harness transcripts are read from: the Run's seam onto its staging.
pub(crate) use pdo_sandbox::adapters::staging::stores::{transcripts_root, HarnessStores};

/// A Run's sandbox, ready to be prepared: its context and the runtime that
/// prepares it. [`Self::ensure_ready`] blocks — call it from `spawn_blocking`.
pub(crate) struct RunSandboxPrep {
    pub(crate) ctx: SandboxContext,
    runtime: SandboxRuntime,
}

impl RunSandboxPrep {
    /// See [`SandboxRuntime::ensure_ready`].
    pub(crate) fn ensure_ready(&self) -> Result<()> {
        self.runtime.ensure_ready(&self.ctx)
    }
}

/// The staging profiles of `db`.
pub(crate) fn profile_store(db: &sqlx::SqlitePool) -> SqliteProfiles {
    SqliteProfiles::new(db.clone())
}

/// The sandbox runtime of this daemon: its docker, the local staged homes and
/// Dockerfiles.
pub(crate) fn runtime(state: &AppState) -> SandboxRuntime {
    local_runtime(state.docker_cmd_override.as_deref())
}

/// `(home_root, sandbox_root)` honouring the per-daemon override (#407 test seam):
/// `Some(dir)` → `(dir, dir/.pdo/sandbox)`; else the real `$HOME`.
pub(crate) fn sandbox_home_roots(state: &AppState) -> Result<(PathBuf, PathBuf)> {
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
/// reads `AppState` / the environment for it.
///
/// Fails (loud) when `$HOME` is unset, the current exe path can't be resolved, or the
/// Run's **frozen** staging-profile selection cannot be resolved (#432) — a sandboxed
/// Run must never fall back to a half-configured container, or to a *different* home
/// content, silently. The one caller that used to swallow this error (boot recovery)
/// turns it into an explicit `RunFailed`.
pub(crate) async fn context_from_state(
    state: &AppState,
    run_state: &RunState,
) -> Result<RunSandboxPrep> {
    let repo_root = crate::effective_repo_root(state, run_state);
    let run_worktree = crate::worktree_ops::worktree_dir_for_run(&repo_root, &run_state.run_id);
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
    let ctx = assemble_context(&profile_store(&state.db), run).await?;
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
pub(crate) async fn merge_back_best_effort(state: &AppState, run_id: &str) {
    let Some((_, run_state)) = crate::reload_run_state(state, run_id).await else {
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
/// entry point for the infra sessions (manager, merge resolver, reattach); `spawn_node`
/// calls [`staging::fill_staging_set`] with the roots it already holds. A no-op (empty
/// env) for `off`, and for a harness with no set. Hard error when the set cannot be
/// copied: a session on an unfillable home would hang on a dialog.
pub(crate) fn stage_harness_set(
    state: &AppState,
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
pub(crate) fn kill_session_best_effort(
    docker_bin: &str,
    sandboxed: bool,
    run_id: &str,
    marker: &str,
) {
    local_runtime(Some(docker_bin)).kill_session_best_effort(
        sandboxed,
        run_id,
        marker,
        host_uid(),
        host_gid(),
    );
}

/// Merge back, destroy the container, purge the staging (`cleanup_run`). Best-effort.
pub(crate) fn cleanup(docker_bin: &str, home_root: &Path, sandbox_root: &Path, run_id: &str) {
    local_runtime(Some(docker_bin)).cleanup(home_root, sandbox_root, run_id);
}

/// The routes of the sandbox context, wired onto this daemon's store, home and
/// referents, asking `host` before every mutation.
pub(crate) fn router(state: &Arc<AppState>, host: Arc<dyn ExtensionHost>) -> Router {
    let profiles = SandboxProfiles::new(
        Arc::new(profile_store(&state.db)),
        host,
        Arc::new(EnvHome::new(state.sandbox_home_override.clone())),
        Arc::new(LegacyReferents {
            db: state.db.clone(),
        }),
    );
    pdo_sandbox::adapters::http::router(Arc::new(profiles))
}

/// Who names a profile, in the stores still owned by the legacy daemon: the instance
/// settings, the Triggers, the Runs.
struct LegacyReferents {
    db: sqlx::SqlitePool,
}

impl ProfileReferents for LegacyReferents {
    fn referents<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Referents> {
        Box::pin(async move {
            Referents {
                instance_default: names_the_instance_default(&self.db, name).await,
                triggers: triggers_naming(&self.db, name).await,
                runs: live_runs_naming(&self.db, name).await,
            }
        })
    }
}

/// The RESOLVED tier, so a stored value shadowed by the env is not reported as a
/// referent when it is not the one that wins.
async fn names_the_instance_default(db: &sqlx::SqlitePool, name: &str) -> bool {
    let stored_default = instance_config::get(db)
        .await
        .ok()
        .and_then(|c| c.default_sandbox);
    sandbox_env::default_sandbox_with(stored_default).profile() == Some(name)
}

async fn triggers_naming(db: &sqlx::SqlitePool, name: &str) -> Vec<TriggerReferent> {
    match trigger_store::list(db).await {
        Ok(list) => list
            .iter()
            .filter(|t| {
                t.sandbox
                    .as_deref()
                    .and_then(SandboxMode::parse)
                    .as_ref()
                    .and_then(SandboxMode::profile)
                    == Some(name)
            })
            .map(|t| TriggerReferent {
                id: t.id.clone(),
                name: t.name.clone(),
                enabled: t.enabled,
            })
            .collect(),
        Err(e) => {
            error!("failed to list triggers for profile referents: {e}");
            Vec::new()
        }
    }
}

/// One indexed pass over `run_started` payloads, excluding any run that already
/// emitted a terminal event. Cheaper than projecting each run, and terminality is
/// exactly "a terminal event exists" for this purpose.
async fn live_runs_naming(db: &sqlx::SqlitePool, name: &str) -> Vec<RunReferent> {
    let rows: Result<Vec<(String, String)>, _> = sqlx::query_as(
        "SELECT e.run_id, e.payload FROM events e \
         WHERE e.kind = 'run_started' AND e.payload IS NOT NULL \
           AND NOT EXISTS ( \
             SELECT 1 FROM events t WHERE t.run_id = e.run_id AND t.kind IN \
               ('run_completed', 'run_failed', 'run_skipped', 'run_halted', 'run_archived') \
           ) \
         ORDER BY e.ts DESC",
    )
    .fetch_all(db)
    .await;
    match rows {
        Ok(rows) => rows
            .iter()
            .filter_map(|(run_id, payload)| {
                let val = serde_json::from_str::<serde_json::Value>(payload).ok()?;
                if val.get("sandbox")?.as_str()? != name {
                    return None;
                }
                Some(RunReferent {
                    run_id: run_id.clone(),
                    pipeline_name: val
                        .get("pipeline_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    name: val.get("name").and_then(|v| v.as_str()).map(str::to_string),
                })
            })
            .collect(),
        Err(e) => {
            error!("failed to query runs for profile referents: {e}");
            Vec::new()
        }
    }
}
