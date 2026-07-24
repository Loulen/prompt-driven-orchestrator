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
//! What this module wires:
//! - [`ensure_ready`] — stage the Claude home once, ensure the image, ensure the
//!   container is up. Called at create-time (eager fail-fast), `boot_recovery`
//!   (reconcile a live sandboxed Run), `open_run_shell` (resurrect), and
//!   `resume_run` (re-arm a terminal Run after a host reboot, #408 D5). Sync and
//!   possibly long (`docker build` on the first machine run) → async callers wrap
//!   it in `spawn_blocking`.
//! - [`cleanup`] — merge the transcripts back, destroy the container, then purge
//!   the staging at `cleanup_run`.
//! - [`transcripts_root`] / [`merge_back_best_effort`] — the observability seam
//!   (#408, ADR-0030 pt 9): cost ([`crate::run_cost`]) and stale-detection
//!   ([`crate::stale_detector`]) read a sandboxed Run's transcripts from its
//!   staged home while it is live, and `merge_back` lands them in `~/.claude`
//!   at the terminal transition (via [`merge_back_best_effort`], the
//!   `append_event` chokepoint) and again at `cleanup_run` (via [`cleanup`],
//!   before `teardown`). session-death detection stays transcript-independent.

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
    /// The Run's pipeline worktree (`-w` cosmetic at create; the trust dialog is
    /// seeded by the staging floor on `repo_root`, the common ancestor of every
    /// worktree, in BOTH sandboxed modes — #426).
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
    /// Where to source the image (#411): resolved once at the edge from
    /// `instance_config` (stored → env → default Registry). Passed to
    /// `sandbox_image::ensure_image` in the sync path — no DB access in the core.
    pub(crate) image_source: sandbox_image::ImageSource,
}

impl SandboxContext {
    /// [`sandbox_staging::Mode`] for this Run, or `None` for `off` (no staging).
    fn staging_mode(&self) -> Option<sandbox_staging::Mode> {
        match self.mode {
            SandboxMode::Off => None,
            SandboxMode::Full => Some(sandbox_staging::Mode::Full),
            SandboxMode::Minimal => Some(sandbox_staging::Mode::Minimal),
        }
    }
}

/// Resolve a [`SandboxContext`] from the daemon state + a projected Run. The one
/// edge function that reads `AppState` / the environment; the core is pure values.
///
/// **Async** (#411): reads a fresh `instance_config` at the boundary to resolve the
/// image source (stored → env → default Registry), so a `PUT /settings` takes effect
/// at the next ensure — consistent with the cap/TTL/model seams. A DB read error is
/// swallowed (falls back env→default), never failing the prep.
///
/// Fails (loud) when `$HOME` is unset or the current exe path can't be resolved —
/// a sandboxed Run must never fall back to a half-configured container silently.
pub(crate) async fn context_from_state(
    state: &AppState,
    run_state: &RunState,
) -> Result<SandboxContext> {
    let repo_root = crate::effective_repo_root(state, run_state);
    let run_worktree = crate::worktree_ops::worktree_dir_for_run(&repo_root, &run_state.run_id);
    // Home root: the per-daemon override (layer-3 harness) wins; else the real
    // `$HOME`. `home_root == host_home` (both the same `$HOME`); the sandbox
    // staging root derives from it (`<home>/.pdo/sandbox`).
    let (home_root, sandbox_root) = sandbox_home_roots(state)?;
    let host_home = home_root.clone();
    let pdo_bin = sandbox_container::pdo_bin_path()?;
    // Read fresh at each prep → a PUT /settings takes effect at the next ensure
    // (ADR-0015), like the cap/TTL/model seams. A DB error falls back env→default,
    // never failing the prep.
    let stored_image_source = crate::instance_config::get(&state.db)
        .await
        .ok()
        .and_then(|c| c.image_source);
    let image_source = sandbox_image::image_source_with(stored_image_source);
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
        image_source,
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

/// The `projects/` root where cost ([`crate::run_cost`]) and stale-detection
/// ([`crate::stale_detector`]) read a Run's Claude Code transcripts (#408). The
/// SINGLE seam shared by both consumers.
///
/// - A sandboxed Run whose **staging still exists** (live / reapable / resumed)
///   → its staged home (`<sandbox_root>/<run_id>/claude-home/projects/`), where
///   `claude` writes in real time through the identity mount.
/// - `off`, OR a sandboxed Run whose staging was purged by `cleanup_run` → the
///   host `~/.claude/projects/`, where `merge_back` flushed the transcripts.
///
/// Dispatch is keyed on the **existence of the staging dir** (not the Run's
/// terminal status): it stays correct even if the best-effort terminal
/// `merge_back` failed, as long as the staging lives (cf. plan #408 D-1). The cwd
/// encoding is unchanged — the caller still resolves the encoded dirname from
/// this base via [`crate::stale_detector::encode_working_dir`], the single source
/// of truth (#373); the seam only swaps the base `projects/` root.
pub(crate) fn transcripts_root(
    mode: SandboxMode,
    run_id: &str,
    home_root: &Path,
    sandbox_root: &Path,
) -> PathBuf {
    if !mode.is_off() && sandbox_staging::staging_dir_for_run(sandbox_root, run_id).exists() {
        sandbox_staging::staged_claude_home(sandbox_root, run_id).join("projects")
    } else {
        home_root.join(".claude").join("projects")
    }
}

/// Merge a Run's staged transcripts back to `~/.claude/projects/` at its terminal
/// transition (#408), so cost + stale-detection see them at the standard encoded
/// dirname once the staging is eventually purged. No-op for `off`.
///
/// Never fails / slows the caller: it re-projects the Run (to read the final
/// `sandbox` mode), resolves the roots, then fires the filesystem walk on a
/// **detached** `spawn_blocking` — the terminal transition must not depend on
/// this merge (ADR-0023). Idempotent (`merge_back` is copy-if-absent-or-larger),
/// so a second terminal event or the later `cleanup_run` merge is safe. The
/// caller (`append_event`) gates on the 4 terminal kinds first, so only terminal
/// events pay the re-projection.
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
    // Fire-and-forget: capture owned values, do NOT `.await` (that would recouple
    // the terminal transition to the merge's latency + JoinError).
    tokio::task::spawn_blocking(move || {
        if let Err(e) = sandbox_staging::merge_back(&home_root, &sandbox_root, &rid) {
            warn!("sandbox merge_back for run {rid} failed (best-effort): {e:#}");
        }
    });
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

    // 1. Stage the Claude home ONCE. The ~1 GB `full` walk must not repeat on
    //    every ensure_ready — gate on the staging dir already existing.
    //    `prepare` then holds the **staging floor** (#426, ADR-0031 §1) in BOTH
    //    sandboxed modes, mode-agnostically: valid credentials, the org managed-
    //    settings baseline, the accepted permissions bypass, trust pre-granted on
    //    `repo_root` (the common ancestor of the pipeline worktree AND every node
    //    sub-worktree), and an empty `projects/` sink. Each guarantee is met either
    //    by a copy of the host file or by a fallback synthesis — which is why an
    //    autonomous Run never blocks on the "trust this folder?", "managed settings
    //    require approval" or bypass-permissions dialogs.
    let staging_dir = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, &ctx.run_id);
    if !staging_dir.exists() {
        let trusted_root = match ctx.mode {
            SandboxMode::Minimal | SandboxMode::Full => Some(ctx.repo_root.as_path()),
            SandboxMode::Off => None,
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

    // 2. Ensure the content-addressed image (`pdo-sandbox:h-<hash>`) exists —
    //    pull-then-retag from GHCR (registry, default), or build it from the seeded
    //    Dockerfile (dockerfile mode / pull fallback), per `ctx.image_source` (#411).
    let image_ref =
        sandbox_image::ensure_image(&ctx.docker_bin, &ctx.sandbox_root, ctx.image_source)
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

/// Merge the transcripts back, destroy the Run's container, then purge its
/// staging (`cleanup_run`, #407 D9 + #408 D-4).
///
/// Best-effort throughout: never fails the archival. **The caller must invoke
/// this BEFORE `git worktree remove`** — the container bind-mounts the repo, so
/// removing a live worktree under it would hit a busy mount. `merge_back` runs
/// **first** ("harvest before purge" is an unskippable invariant): it is
/// idempotent (copy-if-absent-or-larger), so this final pass — which captures any
/// post-terminal growth (resume, late subagent flushes) the detached terminal
/// merge missed — is safe even after that merge already ran.
pub(crate) fn cleanup(docker_bin: &str, home_root: &Path, sandbox_root: &Path, run_id: &str) {
    // Harvest BEFORE teardown wipes the staging. Best-effort (swallow the error).
    let _ = sandbox_staging::merge_back(home_root, sandbox_root, run_id);
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
            // Dockerfile → build-probe path (network-free); keeps the existing
            // ensure_ready assertions (image inspect + build/create) intact (#411).
            image_source: sandbox_image::ImageSource::Dockerfile,
        }
    }

    #[test]
    fn ensure_ready_stages_probes_image_and_container() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Minimal);

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();

        // Staging seeded (minimal → floor only: credentials, remote-settings,
        // settings.json, .claude.json, empty projects/).
        let staging = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, "r1");
        assert!(staging.exists(), "staging dir must be created");
        assert!(
            sandbox_staging::staged_claude_json(&ctx.sandbox_root, "r1").is_file(),
            "minimal staging writes a .claude.json"
        );
        // Docker was probed for image + container (present → no build/create).
        let lines = log_lines(&log);
        assert!(lines.contains(&"image".to_string()), "image inspected");
        assert!(lines.contains(&"container".to_string()), "container probed");
        assert!(
            !lines.contains(&"build".to_string()),
            "present image → no build"
        );
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
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Minimal);

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        // Drop a sentinel into the staging dir; a second ensure_ready must NOT
        // re-run prepare (which would recreate/rewrite the tree).
        let staging = sandbox_staging::staging_dir_for_run(&ctx.sandbox_root, "r1");
        let sentinel = staging.join("SENTINEL");
        std::fs::write(&sentinel, "keep").unwrap();

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        assert!(
            sentinel.exists(),
            "staging must not be re-prepared when present"
        );
    }

    #[test]
    fn ensure_ready_full_seeds_trust_for_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, SandboxMode::Full);

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();

        // #409 D5: full stages a `.claude.json`, and ensure_ready pre-approves the
        // Run's repo_root trust dialog in it — even when the host had no
        // `.claude.json` (an autonomous Run must not block on "trust this folder?").
        let staged = sandbox_staging::staged_claude_json(&ctx.sandbox_root, "r1");
        assert!(staged.is_file(), "full staging writes a .claude.json");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
        let key = ctx.repo_root.to_string_lossy().into_owned();
        assert_eq!(
            json["projects"][&key]["hasTrustDialogAccepted"],
            serde_json::json!(true),
            "full must pre-approve the repo_root trust dialog: {json}"
        );

        // #426: the staging floor runs through the REAL caller, not just a direct
        // `prepare` call. `test_ctx`'s home carries credentials only, so G3 lands on
        // its synthesis branch.
        let staged_settings =
            sandbox_staging::staged_claude_home(&ctx.sandbox_root, "r1").join("settings.json");
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged_settings).unwrap()).unwrap();
        assert_eq!(
            settings["skipDangerousModePermissionPrompt"],
            serde_json::json!(true),
            "ensure_ready must hold the staging floor: {settings}"
        );
    }

    #[test]
    fn cleanup_removes_container_and_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let home_root = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Seed a staging dir to be torn down.
        std::fs::create_dir_all(sandbox_staging::staged_claude_home(&sandbox_root, "r1")).unwrap();
        assert!(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1").exists());

        // `cleanup` is idempotent + best-effort (swallows ETXTBSY); retry until the
        // container-remove is logged.
        let logged = retry_side_effect(
            || cleanup(&docker, &home_root, &sandbox_root, "r1"),
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
    fn kill_session_minimal_execs_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let logged = retry_side_effect(
            || kill_session_best_effort(&docker, SandboxMode::Minimal, "r1", "pdo-r1-n1-iter-1"),
            || {
                let l = log_lines(&log);
                !l.is_empty()
                    && l[0] == "exec"
                    && l.iter()
                        .any(|x| x.contains("PDO_SBX_SESSION=pdo-r1-n1-iter-1"))
            },
        );
        assert!(
            logged,
            "targeted kill must exec with the session marker; log: {:?}",
            log_lines(&log)
        );
    }

    // --- #408: cleanup harvests transcripts before teardown --------------------

    #[test]
    fn cleanup_merges_transcripts_before_purging_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _log) = write_fake_docker(tmp.path());
        let home_root = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Seed a staged transcript under one encoded project dir.
        let staged_projects =
            sandbox_staging::staged_claude_home(&sandbox_root, "r1").join("projects");
        let proj = staged_projects.join("-enc-worktree");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("s.jsonl"), "{\"line\":1}\n").unwrap();

        retry_side_effect(
            || cleanup(&docker, &home_root, &sandbox_root, "r1"),
            || {
                home_root
                    .join(".claude/projects/-enc-worktree/s.jsonl")
                    .is_file()
            },
        );

        // merge_back landed the transcript in the host projects dir …
        assert!(
            home_root
                .join(".claude/projects/-enc-worktree/s.jsonl")
                .is_file(),
            "cleanup must merge transcripts to the host BEFORE teardown"
        );
        // … and teardown then purged the staging.
        assert!(
            !sandbox_staging::staging_dir_for_run(&sandbox_root, "r1").exists(),
            "cleanup purges the staging after the merge"
        );
    }

    // --- #408: transcripts_root seam (3 arms + root-invariance) ----------------

    #[test]
    fn transcripts_root_off_is_always_host() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Even if a staging dir happens to exist, `off` reads the host.
        std::fs::create_dir_all(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        assert_eq!(
            transcripts_root(SandboxMode::Off, "r1", &home, &sandbox_root),
            home.join(".claude").join("projects")
        );
    }

    #[test]
    fn transcripts_root_sandboxed_live_is_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Staging present → live/reapable Run → read the staged home.
        std::fs::create_dir_all(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        assert_eq!(
            transcripts_root(SandboxMode::Minimal, "r1", &home, &sandbox_root),
            sandbox_staging::staged_claude_home(&sandbox_root, "r1").join("projects")
        );
        // `full` behaves identically.
        assert_eq!(
            transcripts_root(SandboxMode::Full, "r1", &home, &sandbox_root),
            sandbox_staging::staged_claude_home(&sandbox_root, "r1").join("projects")
        );
    }

    #[test]
    fn transcripts_root_sandboxed_after_cleanup_is_host() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // No staging dir (post-`cleanup_run`) → read the host, where merge_back
        // flushed. Keyed on staging EXISTENCE, not the Run's terminal status.
        assert!(!sandbox_staging::staging_dir_for_run(&sandbox_root, "r1").exists());
        assert_eq!(
            transcripts_root(SandboxMode::Minimal, "r1", &home, &sandbox_root),
            home.join(".claude").join("projects")
        );
    }

    #[test]
    fn transcripts_root_encoding_is_root_invariant() {
        // AC5: the seam only swaps the base `projects/` root — the per-cwd encoded
        // segment stays the single source of truth (`encode_working_dir`, #373),
        // never re-derived by the seam. So for a given cwd, appending the encoded
        // segment to the seam-resolved root yields the same dirname regardless of
        // which base (host vs staging) the seam picked.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        let cwd = std::path::Path::new("/home/u/.pdo/runs/r1/worktree");
        let enc = crate::stale_detector::encode_working_dir(cwd);

        // Host arm (no staging).
        let host = transcripts_root(SandboxMode::Minimal, "r1", &home, &sandbox_root);
        assert_eq!(host.join(&enc), home.join(".claude/projects").join(&enc));

        // Staging arm (staging materialised) — same encoded segment appended.
        std::fs::create_dir_all(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        let staged = transcripts_root(SandboxMode::Minimal, "r1", &home, &sandbox_root);
        assert_eq!(
            staged.join(&enc),
            sandbox_staging::staged_claude_home(&sandbox_root, "r1")
                .join("projects")
                .join(&enc)
        );
        // The bases differ, but the trailing encoded segment is identical.
        assert_eq!(host.file_name(), staged.file_name()); // both end in "projects"
        assert_eq!(
            host.join(&enc).file_name(),
            staged.join(&enc).file_name(),
            "the encoded cwd segment is root-invariant"
        );
    }
}
