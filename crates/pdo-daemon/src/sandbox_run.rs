//! Run-advance wiring for the sandbox (#407, slice D / tracer bullet of PRD #403).
//!
//! The three pure modules — [`crate::sandbox_staging`] (#404),
//! [`crate::sandbox_image`] (#405), [`crate::sandbox_container`] (#406) — each own
//! one facet (home staging / image / container) and read no `AppState`. This
//! module is the thin **orchestration layer** that consumes them: it assembles a
//! [`SandboxContext`] value at the daemon boundary ([`context_from_state`], the
//! only reader of `AppState`), then the core ([`ensure_ready`], [`cleanup`]) works
//! from explicit values only — mirroring the pure-module discipline.
//!
//! What #407 wires (and what it does NOT):
//! - [`ensure_ready`] — stage the Claude home once, ensure the image, ensure the
//!   container is up. Called at create-time (eager fail-fast), `boot_recovery`
//!   (reconcile a live sandboxed Run), and `open_run_shell` (resurrect). Sync and
//!   possibly long (`docker build` on the first machine run) → async callers wrap
//!   it in `spawn_blocking`.
//! - [`cleanup`] — destroy the container + purge the staging at `cleanup_run`.
//!   **`merge_back` is NOT wired here** — it belongs to the observability slice
//!   #408 (together with the `transcripts_root` seam), so a `pure`/`copy` Run is
//!   deliberately blind to cost / stale-detection until then. That is an
//!   *observability* gap, not a correctness/liveness one: session-death detection
//!   is transcript-independent and stays alive.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::warn;

use crate::event_log::{RunState, SandboxMode};
use crate::{sandbox_container, sandbox_image, sandbox_staging, AppState};

/// Everything [`ensure_ready`] / [`cleanup`] need, assembled once at the boundary
/// by [`context_from_state`] from `AppState` + the projected `RunState`. Holds
/// owned values so the core never reaches back into `AppState`.
pub(crate) struct SandboxContext {
    /// The `docker` binary to invoke (`state.docker_cmd_override` → `"docker"`).
    pub(crate) docker_bin: String,
    pub(crate) run_id: String,
    pub(crate) mode: SandboxMode,
    /// Effective repo root — bind-mounted rw at its host path. One mount covers
    /// the repo + every node sub-worktree under `.pdo/runs/` + `.pdo/prompts`.
    pub(crate) repo_root: PathBuf,
    /// The Run's pipeline worktree (`-w` cosmetic at create; the `pure` trust
    /// dialog is seeded on `repo_root`, the common ancestor of every worktree).
    pub(crate) run_worktree: PathBuf,
    pub(crate) daemon_port: u16,
    /// Host `$HOME` — source of `.claude` for `prepare`.
    pub(crate) home_root: PathBuf,
    /// `$HOME/.pdo/sandbox` — per-Run staging lives under `<sandbox_root>/<run>`.
    pub(crate) sandbox_root: PathBuf,
    /// Host `$HOME` again, as the mount-target root for `.claude`/`.claude.json`
    /// inside the container (kept distinct from `home_root` to mirror the two
    /// pure-module params it feeds, even though both resolve to `$HOME`).
    pub(crate) host_home: PathBuf,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    /// Host `pdo` binary, bind-mounted read-only at `/usr/local/bin/pdo`.
    pub(crate) pdo_bin: PathBuf,
}

impl SandboxContext {
    /// [`sandbox_staging::Mode`] for this Run, or `None` for `off` (no staging).
    fn staging_mode(&self) -> Option<sandbox_staging::Mode> {
        match self.mode {
            SandboxMode::Off => None,
            SandboxMode::Copy => Some(sandbox_staging::Mode::Copy),
            SandboxMode::Pure => Some(sandbox_staging::Mode::Pure),
        }
    }
}

/// Resolve a [`SandboxContext`] from the daemon state + a projected Run. The one
/// edge function that reads `AppState` / the environment; the core is pure values.
///
/// Fails (loud) when `$HOME` is unset or the current exe path can't be resolved —
/// a sandboxed Run must never fall back to a half-configured container silently.
pub(crate) fn context_from_state(state: &AppState, run_state: &RunState) -> Result<SandboxContext> {
    let repo_root = crate::effective_repo_root(state, run_state);
    let run_worktree = crate::worktree_ops::worktree_dir_for_run(&repo_root, &run_state.run_id);
    // Home root: the per-daemon override (layer-3 harness) wins; else the real
    // `$HOME`. `home_root == host_home` (both the same `$HOME`); the sandbox
    // staging root derives from it (`<home>/.pdo/sandbox`).
    let (home_root, sandbox_root) = sandbox_home_roots(state)?;
    let host_home = home_root.clone();
    let pdo_bin = sandbox_container::pdo_bin_path()?;
    Ok(SandboxContext {
        docker_bin: docker_bin(state),
        run_id: run_state.run_id.clone(),
        mode: run_state.sandbox,
        repo_root,
        run_worktree,
        daemon_port: state.port,
        home_root,
        sandbox_root,
        host_home,
        uid: sandbox_container::host_uid(),
        gid: sandbox_container::host_gid(),
        pdo_bin,
    })
}

/// `(home_root, sandbox_root)` honouring the per-daemon override (#407 test seam):
/// `Some(dir)` → `(dir, dir/.pdo/sandbox)`; else the real `$HOME` via
/// [`sandbox_staging::default_roots_from_env`].
pub(crate) fn sandbox_home_roots(state: &AppState) -> Result<(PathBuf, PathBuf)> {
    if let Some(home) = &state.sandbox_home_override {
        let sandbox_root = home.join(".pdo").join("sandbox");
        return Ok((home.clone(), sandbox_root));
    }
    sandbox_staging::default_roots_from_env()
        .context("HOME is not set; cannot resolve the sandbox staging root")
}

/// The `docker` binary this daemon uses (per-daemon override → `"docker"`).
pub(crate) fn docker_bin(state: &AppState) -> String {
    state
        .docker_cmd_override
        .clone()
        .unwrap_or_else(|| "docker".to_string())
}

/// Guarantee the Run's sandbox is ready to accept `docker exec` tails: staged
/// home present, image built, container up. Idempotent — safe to call at
/// create-time, boot recovery, spawn-time, and run-shell open.
///
/// **Sync and potentially long** (`docker build` on the first machine run):
/// async callers MUST wrap it in `tokio::task::spawn_blocking` so the executor
/// isn't blocked. `off` is a defensive no-op (callers already gate on
/// `!sandbox.is_off()`).
pub(crate) fn ensure_ready(ctx: &SandboxContext) -> Result<()> {
    let Some(mode) = ctx.staging_mode() else {
        return Ok(()); // off: gated by callers; no docker touched.
    };

    // 1. Stage the Claude home ONCE. The ~98 MB `copy` walk (and the `pure` seed)
    //    must not repeat on every ensure_ready — gate on the staging dir already
    //    existing. `pure` pre-approves the trust dialog on `repo_root`, the common
    //    ancestor of the pipeline worktree AND every node sub-worktree.
    let staging_dir = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, &ctx.run_id);
    if !staging_dir.exists() {
        let trusted_root = match ctx.mode {
            SandboxMode::Pure => Some(ctx.repo_root.as_path()),
            _ => None,
        };
        sandbox_staging::prepare(
            &ctx.home_root,
            &ctx.sandbox_root,
            mode,
            &ctx.run_id,
            trusted_root,
        )
        .with_context(|| format!("failed to stage the sandbox home for run {}", ctx.run_id))?;
    }

    // 2. Ensure the content-addressed image (`pdo-sandbox:h-<hash>`) exists,
    //    building it from the seeded Dockerfile on the first machine run.
    let image_ref = sandbox_image::ensure_image(&ctx.docker_bin, &ctx.sandbox_root)
        .context("failed to ensure the sandbox image")?;

    // 3. Assemble the container spec + ensure the long-lived container is up.
    let staged_home = sandbox_staging::staged_claude_home(&ctx.sandbox_root, &ctx.run_id);
    let staged_json = sandbox_staging::staged_claude_json(&ctx.sandbox_root, &ctx.run_id);
    let spec = sandbox_container::ContainerSpec {
        image_ref: &image_ref,
        repo_root: &ctx.repo_root,
        run_worktree: &ctx.run_worktree,
        staged_home: &staged_home,
        staged_json: &staged_json,
        pdo_bin: &ctx.pdo_bin,
        host_home: &ctx.host_home,
        uid: ctx.uid,
        gid: ctx.gid,
        daemon_port: ctx.daemon_port,
    };
    sandbox_container::ensure_running(&ctx.docker_bin, &ctx.run_id, &spec)
        .context("failed to ensure the sandbox container is running")?;

    Ok(())
}

/// Destroy the Run's container and purge its staging (`cleanup_run`, #407 D9).
///
/// Best-effort: never fails the archival. **The caller must invoke this BEFORE
/// `git worktree remove`** — the container bind-mounts the repo, so removing a
/// live worktree under it would hit a busy mount. `merge_back` is intentionally
/// absent (→ #408).
pub(crate) fn cleanup(docker_bin: &str, sandbox_root: &Path, run_id: &str) {
    if let Err(e) = sandbox_container::remove(docker_bin, run_id) {
        warn!("sandbox cleanup: failed to remove container for run {run_id} (best-effort): {e:#}");
    }
    // `teardown` is already best-effort (swallows fs errors); log nothing extra.
    let _ = sandbox_staging::teardown(sandbox_root, run_id);
}

/// Best-effort **targeted** kill of one session's process tree inside the Run's
/// container (#407 D8). No-op for `off`. The `docker exec` client killed on the
/// tmux side does NOT kill the container process (reparented onto PID 1), so this
/// separate exec scans `/proc/*/environ` for the session marker and signals only
/// the matching tree — sibling sessions survive.
pub(crate) fn kill_session_best_effort(
    docker_bin: &str,
    sandbox: SandboxMode,
    run_id: &str,
    marker: &str,
) {
    if sandbox.is_off() {
        return;
    }
    if let Err(e) = sandbox_container::kill_session_in_container(
        docker_bin,
        run_id,
        marker,
        sandbox_container::host_uid(),
        sandbox_container::host_gid(),
    ) {
        warn!(
            "sandbox targeted kill of session {marker} in run {run_id} failed (best-effort): {e:#}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn q(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// A fake `docker` that logs argv and canned-responds so `ensure_ready`
    /// reaches its container step without a real daemon: `image inspect` → exit 0
    /// (image present, no build), `container inspect` → `true` (up, no create).
    /// Mirrors the per-module fakes (no `std::env` mutation — threaded as
    /// `docker_bin`).
    fn write_fake_docker(dir: &Path) -> (String, PathBuf) {
        let bin = dir.join("fake-docker");
        let log = dir.join("argv.log");
        let script = format!(
            "#!/usr/bin/env bash\n\
             printf '%s\\n' \"$@\" >> {log}\n\
             case \"$1\" in\n\
             image) exit 0 ;;\n\
             container) printf 'true'; exit 0 ;;\n\
             *) exit 0 ;;\n\
             esac\n",
            log = q(&log.display().to_string()),
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin.to_str().unwrap().to_string(), log)
    }

    fn log_lines(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Retry an op that returns `Result` on `ETXTBSY` (os error 26): exec-ing a
    /// freshly-written fake binary can transiently race a sibling test's
    /// fork/exec (rust-lang/rust#45719). Mirrors the guard in `sandbox_image` /
    /// `sandbox_container` tests.
    fn retry_etxtbsy<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
        for _ in 0..100 {
            match op() {
                Err(e)
                    if e.chain().any(|c| {
                        c.downcast_ref::<std::io::Error>()
                            .and_then(std::io::Error::raw_os_error)
                            == Some(26)
                    }) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                other => return other,
            }
        }
        op()
    }

    /// Run a best-effort side-effect op (returns `()`, swallows ETXTBSY) until a
    /// predicate on the log holds — re-invoking on the transient exec race. The
    /// ops here (`cleanup`/`kill`) are idempotent, so a re-invocation is safe.
    fn retry_side_effect(mut op: impl FnMut(), mut ready: impl FnMut() -> bool) -> bool {
        for _ in 0..100 {
            op();
            if ready() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        ready()
    }

    /// Build a context rooted under temp dirs (bypasses the env/exe resolvers).
    fn test_ctx(tmp: &Path, docker_bin: String, mode: SandboxMode) -> SandboxContext {
        let home = tmp.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
        SandboxContext {
            docker_bin,
            run_id: "r1".to_string(),
            mode,
            repo_root: tmp.join("repo"),
            run_worktree: tmp.join("repo/.pdo/runs/r1/worktree"),
            daemon_port: 6172,
            home_root: home.clone(),
            sandbox_root: tmp.join("sandbox"),
            host_home: home,
            uid: 1000,
            gid: 1000,
            pdo_bin: tmp.join("pdo"),
        }
    }

    #[test]
    fn ensure_ready_stages_probes_image_and_container() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Pure);

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();

        // Staging seeded (pure → credentials + .claude.json + empty projects/).
        let staging = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, "r1");
        assert!(staging.exists(), "staging dir must be created");
        assert!(
            sandbox_staging::staged_claude_json(&ctx.sandbox_root, "r1").is_file(),
            "pure staging writes a .claude.json"
        );
        // Docker was probed for image + container (present → no build/create).
        let lines = log_lines(&log);
        assert!(lines.contains(&"image".to_string()), "image inspected");
        assert!(lines.contains(&"container".to_string()), "container probed");
        assert!(!lines.contains(&"build".to_string()), "present image → no build");
        assert!(
            !lines.contains(&"create".to_string()),
            "running container → no create"
        );
    }

    #[test]
    fn ensure_ready_off_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Off);

        ensure_ready(&ctx).unwrap();

        assert!(
            !tmp.path().join("sandbox").exists(),
            "off must not stage anything"
        );
        assert!(!log.exists(), "off must not invoke docker");
    }

    #[test]
    fn ensure_ready_does_not_restage_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Pure);

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        // Drop a sentinel into the staging dir; a second ensure_ready must NOT
        // re-run prepare (which would recreate/rewrite the tree).
        let staging = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, "r1");
        let sentinel = staging.join("SENTINEL");
        std::fs::write(&sentinel, "keep").unwrap();

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        assert!(sentinel.exists(), "staging must not be re-prepared when present");
    }

    #[test]
    fn cleanup_removes_container_and_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let sandbox_root = tmp.path().join("sandbox");
        // Seed a staging dir to be torn down.
        std::fs::create_dir_all(sandbox_staging::staged_claude_home(&sandbox_root, "r1")).unwrap();
        assert!(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1").exists());

        // `cleanup` is idempotent + best-effort (swallows ETXTBSY); retry until the
        // container-remove is logged.
        let logged = retry_side_effect(
            || cleanup(&docker, &sandbox_root, "r1"),
            || {
                let l = log_lines(&log);
                l.len() >= 3 && l[..3] == ["rm", "-f", "pdo-sbx-r1"]
            },
        );
        assert!(
            logged,
            "cleanup removes the container; log: {:?}",
            log_lines(&log)
        );
        assert!(
            !sandbox_staging::staging_dir_for_run(&sandbox_root, "r1").exists(),
            "cleanup purges the staging"
        );
    }

    #[test]
    fn kill_session_off_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        kill_session_best_effort(&docker, SandboxMode::Off, "r1", "pdo-r1-n1-iter-1");
        assert!(!log.exists(), "off must not invoke docker to kill");
    }

    #[test]
    fn kill_session_pure_execs_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let logged = retry_side_effect(
            || kill_session_best_effort(&docker, SandboxMode::Pure, "r1", "pdo-r1-n1-iter-1"),
            || {
                let l = log_lines(&log);
                !l.is_empty()
                    && l[0] == "exec"
                    && l.iter().any(|x| x.contains("PDO_SBX_SESSION=pdo-r1-n1-iter-1"))
            },
        );
        assert!(
            logged,
            "targeted kill must exec with the session marker; log: {:?}",
            log_lines(&log)
        );
    }
}
