//! Prepare, clean up and reach into a Run's sandbox (#407): the orchestration of
//! the staged home, the image and the container.
//!
//! Callers of [`SandboxRuntime::ensure_ready`]: create-time (eager fail-fast), boot
//! recovery (reconcile a live sandboxed Run), the run shell (resurrect), and resume
//! (re-arm a terminal Run after a host reboot, #408 D5).

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::container::{
    binary_present_in_container, ensure_running, kill_session_in_container, remove,
};
use super::context::SandboxContext;
use super::image::ensure_image;
use super::ports::{DockerCli, DockerfileStore, HomeStaging};
use crate::domain::container::ContainerSpec;
use crate::domain::image::ImagePlan;
use crate::domain::profile::env_names;
use crate::domain::staging::{harness_home_mounts, staged_claude_home, staged_claude_json};

/// The ports a sandbox needs at run time: docker, the staged homes, the Dockerfiles.
#[derive(Clone)]
pub struct SandboxRuntime {
    pub docker: Arc<dyn DockerCli>,
    pub staging: Arc<dyn HomeStaging>,
    pub dockerfiles: Arc<dyn DockerfileStore>,
}

impl SandboxRuntime {
    /// Guarantee the Run's sandbox is ready to accept `docker exec` tails: staged home
    /// present, image built, container up. Idempotent — safe to call at create-time,
    /// boot recovery, spawn-time, and run-shell open.
    ///
    /// **Sync and potentially long** (`docker build` on the first machine run): async
    /// callers MUST wrap it in `tokio::task::spawn_blocking` so the executor isn't
    /// blocked. `off` is a defensive no-op (callers already gate on `!sandbox.is_off()`).
    pub fn ensure_ready(&self, ctx: &SandboxContext) -> Result<()> {
        let Some(entries) = ctx.entries.as_deref() else {
            return Ok(()); // off: gated by callers; no docker touched.
        };

        // 1. Stage the profile ONCE. The ~1 GB `full` walk must not repeat on every
        //    ensure_ready — gate on the staging dir already existing. No harness
        //    **staging set** is applied here (#708, ADR-0063 §3): `claude`'s five
        //    guarantees and `pi`'s home are copied at the spawn of the first session of
        //    the Run that resolves that harness — so a claude-only Run never carries
        //    pi's auth, and vice versa.
        if !self.staging.exists(&ctx.sandbox_root, &ctx.run_id) {
            // Name the profile AND the list once per staging, so "why does this Run
            // carry plugins/" is answerable from the log rather than by reading the DB
            // (which may have been edited since — the list is frozen, the profile is not).
            info!(
                "sandbox: staging run {} with profile `{}` ({} entries: {})",
                ctx.run_id,
                ctx.mode.as_str(),
                entries.len(),
                entries.join(", ")
            );
            // #468: the env gets the SAME once-per-staging visibility and a DIFFERENT
            // rule — the **names**, never the values. A `$HOME`-relative path is not a
            // secret; an env value routinely is, and the systemd journal outlives the
            // Run. `env_names` is a named function precisely so that rule has a test.
            if !ctx.env.is_empty() {
                info!(
                    "sandbox: run {} carries {} profile env var(s) (names only: {})",
                    ctx.run_id,
                    ctx.env.len(),
                    env_names(&ctx.env)
                );
            }
            self.staging
                .prepare(&ctx.home_root, &ctx.sandbox_root, entries, &ctx.run_id)
                .with_context(|| {
                    format!("failed to stage the sandbox home for run {}", ctx.run_id)
                })?;
        }
        // #708 / ADR-0063 §3: the sources of the fixed mounts exist. Unconditional
        // (cheap, idempotent): a staging prepared by a pre-#708 daemon lacks the `pi`
        // root, and a `docker create` whose mount source is missing yields a root-owned
        // directory (rule M1 of `extra_mounts`).
        self.staging
            .ensure_mount_anchors(&ctx.sandbox_root, &ctx.run_id)
            .with_context(|| {
                format!("failed to anchor the sandbox mounts for run {}", ctx.run_id)
            })?;

        // 2. Ensure the Run's image exists locally, per the plan resolved at assembly:
        //    pull-then-retag `<name>:h-<hash>` or build it from the RESOLVED Dockerfile —
        //    OR pull a profile's EXPLICIT registry ref, where a failed pull is a hard error
        //    and no build is attempted (#467, ADR-0030 pt 7 as amended).
        let image_ref = ensure_image(
            self.docker.as_ref(),
            self.dockerfiles.as_ref(),
            &ctx.sandbox_root,
            &ctx.image_plan,
        )
        .context("failed to ensure the sandbox image")?;
        // Name the image ONCE per prep, so "which image did this Run actually get" is
        // answerable from the log: the source is frozen, the profile is not.
        info!(
            "sandbox: run {} uses image {image_ref} (source: {})",
            ctx.run_id,
            match &ctx.image_plan {
                ImagePlan::ExplicitRef { profile, .. } =>
                    format!("explicit ref from profile `{profile}`"),
                ImagePlan::HashDerived { dockerfile, .. } => format!(
                    "content hash of {} (`{}` tier)",
                    dockerfile.path.display(),
                    dockerfile.source.as_str()
                ),
            }
        );

        // 3. Assemble the container spec + ensure the long-lived container is up.
        let staged_home = staged_claude_home(&ctx.sandbox_root, &ctx.run_id);
        let staged_json = staged_claude_json(&ctx.sandbox_root, &ctx.run_id);
        // #432: derived from `frozen list × disk`, NOT returned by `prepare` — `prepare`
        // is skipped whenever the staging already exists.
        let extra =
            self.staging
                .extra_mounts(&ctx.sandbox_root, &ctx.run_id, &ctx.host_home, entries);
        // #708: the empty home root of every first-party harness whose root is not
        // already a fixed mount (`pi`'s `.pi/agent`; `claude`'s is `claude-home`).
        let harness_homes = harness_home_mounts(&ctx.sandbox_root, &ctx.run_id, &ctx.host_home);
        let spec = ContainerSpec {
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
            extra_mounts: &extra,
            harness_homes: &harness_homes,
            // #468: the FROZEN env. `ensure_running` only consults the spec on its
            // `Absent` arm — `docker start` never re-evaluates a pre-existing container's
            // env, any more than its mounts (ADR-0031 §6/§8, guaranteed twice).
            env: &ctx.env,
            // ADR-0047: the `.git` of each writable secondary, mounted rw so `git` works
            // inside it. Same freeze caveat as the mounts above.
            writable_secondary_gitdirs: &ctx.writable_secondary_gitdirs,
        };
        ensure_running(self.docker.as_ref(), &ctx.run_id, &spec)
            .context("failed to ensure the sandbox container is running")?;

        Ok(())
    }

    /// Merge the transcripts back, destroy the Run's container, then purge its staging
    /// (`cleanup_run`, #407 D9 + #408 D-4).
    ///
    /// Best-effort throughout: never fails the archival. **The caller must invoke this
    /// BEFORE `git worktree remove`** — the container bind-mounts the repo. `merge_back`
    /// runs **first** ("harvest before purge" is an unskippable invariant): it is
    /// idempotent (copy-if-absent-or-larger), so this final pass is safe even after the
    /// terminal merge already ran.
    pub fn cleanup(&self, home_root: &Path, sandbox_root: &Path, run_id: &str) {
        // Harvest BEFORE teardown wipes the staging. Best-effort (swallow the error).
        let _ = self.staging.merge_back(home_root, sandbox_root, run_id);
        if let Err(e) = remove(self.docker.as_ref(), run_id) {
            warn!(
                "sandbox cleanup: failed to remove container for run {run_id} (best-effort): {e:#}"
            );
        }
        // `teardown` is already best-effort (swallows fs errors); log nothing extra.
        let _ = self.staging.teardown(sandbox_root, run_id);
    }

    /// Destroy the Run's container (`docker rm -f`). Idempotent.
    pub fn remove_container(&self, run_id: &str) -> Result<()> {
        remove(self.docker.as_ref(), run_id)
    }

    /// Whether `binary` resolves inside the Run's container (#708, ADR-0063 §5).
    pub fn binary_present(&self, run_id: &str, uid: u32, gid: u32, binary: &str) -> Result<bool> {
        binary_present_in_container(self.docker.as_ref(), run_id, uid, gid, binary)
    }

    /// Best-effort **targeted** kill of one session's process tree inside the Run's
    /// container (#407 D8). No-op for `off`. The `docker exec` client killed on the tmux
    /// side does NOT kill the container process (reparented onto PID 1), so this
    /// separate exec scans `/proc/*/environ` for the session marker and signals only the
    /// matching tree — sibling sessions survive.
    pub fn kill_session_best_effort(
        &self,
        sandboxed: bool,
        run_id: &str,
        marker: &str,
        uid: u32,
        gid: u32,
    ) {
        if !sandboxed {
            return;
        }
        if let Err(e) = kill_session_in_container(self.docker.as_ref(), run_id, marker, uid, gid) {
            warn!(
                "sandbox targeted kill of session {marker} in run {run_id} failed (best-effort): {e:#}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::host::{host_gid, host_uid};
    use crate::adapters::local_runtime;
    use crate::adapters::staging;
    use crate::domain::image::{self as sandbox_image};
    use crate::domain::mode::SandboxMode;
    use crate::domain::profile::{base_entries, resolve_entry_list, FULL_PROFILE, MINIMAL_PROFILE};
    use crate::domain::staging::staging_dir_for_run;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    // The cases below drive the runtime through the real adapters (a fake `docker`
    // executable threaded as the binary, the real filesystem); the in-memory cases at
    // the bottom pin the orchestration alone.

    /// A context, and the fake docker binary its runtime runs.
    struct Ctx {
        inner: SandboxContext,
        docker: String,
    }

    impl std::ops::Deref for Ctx {
        type Target = SandboxContext;
        fn deref(&self) -> &SandboxContext {
            &self.inner
        }
    }

    fn ensure_ready(ctx: &Ctx) -> Result<()> {
        local_runtime(Some(&ctx.docker)).ensure_ready(&ctx.inner)
    }

    fn cleanup(docker_bin: &str, home_root: &Path, sandbox_root: &Path, run_id: &str) {
        local_runtime(Some(docker_bin)).cleanup(home_root, sandbox_root, run_id)
    }

    fn kill_session_best_effort(docker_bin: &str, sandboxed: bool, run_id: &str, marker: &str) {
        local_runtime(Some(docker_bin)).kill_session_best_effort(
            sandboxed,
            run_id,
            marker,
            host_uid(),
            host_gid(),
        )
    }

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

    /// Like [`write_fake_docker`] but `container inspect` reports **absent**, so
    /// `ensure_running` reaches `docker create` — which is the only place the env `-e` are
    /// observable (#468). The default fake answers `true` (up) and would skip the create.
    fn write_fake_docker_absent_container(dir: &Path) -> (String, PathBuf) {
        let bin = dir.join("fake-docker-absent");
        let log = dir.join("argv-absent.log");
        let script = format!(
            "#!/usr/bin/env bash\n\
             printf '%s\\n' \"$@\" >> {log}\n\
             case \"$1\" in\n\
             image) exit 0 ;;\n\
             container) printf 'Error: No such container' >&2; exit 1 ;;\n\
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

    fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
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

    fn full() -> SandboxMode {
        SandboxMode::Profile(FULL_PROFILE.into())
    }
    fn minimal() -> SandboxMode {
        SandboxMode::Profile(MINIMAL_PROFILE.into())
    }

    /// Build a context rooted under temp dirs (bypasses the env/exe resolvers).
    ///
    /// #432: `mode` names the profile and `entries` is the FROZEN list — the two are
    /// separate parameters here because the point of the freeze is that they *can*
    /// disagree with whatever the store now says. `Off` carries `None`.
    fn test_ctx(tmp: &Path, docker_bin: String, mode: SandboxMode) -> Ctx {
        let entries = match mode.profile() {
            None => None,
            Some(name) => {
                let base = base_entries(name);
                Some(resolve_entry_list(&base, &[], &[]).entries)
            }
        };
        test_ctx_with_entries(tmp, docker_bin, mode, entries)
    }

    /// Like [`test_ctx`] but with an explicit frozen entry list.
    fn test_ctx_with_entries(
        tmp: &Path,
        docker_bin: String,
        mode: SandboxMode,
        entries: Option<Vec<String>>,
    ) -> Ctx {
        test_ctx_with(tmp, docker_bin, mode, entries, BTreeMap::new())
    }

    /// Like [`test_ctx_with_entries`] but with an explicit frozen env too (#468). The image plan
    /// is the historical one — see [`test_ctx_full`] for the #467 variants.
    fn test_ctx_with(
        tmp: &Path,
        docker_bin: String,
        mode: SandboxMode,
        entries: Option<Vec<String>>,
        env: BTreeMap<String, String>,
    ) -> Ctx {
        let sandbox_root = tmp.join("sandbox");
        test_ctx_full(
            tmp,
            docker_bin,
            mode,
            entries,
            env,
            // Dockerfile → build-probe path (network-free); keeps the existing ensure_ready
            // assertions (image inspect + build/create) intact (#411). The seeded default
            // location / `default` tier — the pre-#431, pre-#467 input.
            sandbox_image::ImagePlan::HashDerived {
                dockerfile: sandbox_image::resolve_dockerfile(None, None, &sandbox_root),
                source: sandbox_image::ImageSource::Dockerfile,
            },
        )
    }

    /// Like [`test_ctx_with`] but with an explicit image plan too (#467).
    fn test_ctx_full(
        tmp: &Path,
        docker_bin: String,
        mode: SandboxMode,
        entries: Option<Vec<String>>,
        env: BTreeMap<String, String>,
        image_plan: sandbox_image::ImagePlan,
    ) -> Ctx {
        let home = tmp.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
        let sandbox_root = tmp.join("sandbox");
        let inner = SandboxContext {
            run_id: "r1".to_string(),
            mode,
            entries,
            env,
            repo_root: tmp.join("repo"),
            run_worktree: tmp.join("repo/.pdo/runs/r1/worktree"),
            daemon_port: 6172,
            home_root: home.clone(),
            sandbox_root: sandbox_root.clone(),
            host_home: home,
            uid: 1000,
            gid: 1000,
            pdo_bin: tmp.join("pdo"),
            image_plan,
            // No writable secondaries in these unit fixtures (ADR-0047).
            writable_secondary_gitdirs: Vec::new(),
        };
        Ctx {
            inner,
            docker: docker_bin,
        }
    }

    #[test]
    fn ensure_ready_stages_probes_image_and_container() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, minimal());

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();

        // Staging seeded (minimal → floor only: credentials, remote-settings,
        // settings.json, .claude.json, empty projects/).
        let staging = staging_dir_for_run(&ctx.sandbox_root, "r1");
        assert!(staging.exists(), "staging dir must be created");
        assert!(
            staged_claude_json(&ctx.sandbox_root, "r1").is_file(),
            "minimal staging writes a .claude.json"
        );
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
        let ctx = test_ctx(tmp.path(), docker, minimal());

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        // Drop a sentinel into the staging dir; a second ensure_ready must NOT
        // re-run prepare (which would recreate/rewrite the tree).
        let staging = staging_dir_for_run(&ctx.sandbox_root, "r1");
        let sentinel = staging.join("SENTINEL");
        std::fs::write(&sentinel, "keep").unwrap();

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        assert!(
            sentinel.exists(),
            "staging must not be re-prepared when present"
        );
    }

    /// #708 / ADR-0063 §3: `ensure_ready` no longer applies `claude`'s staging set —
    /// it anchors an EMPTY `.claude.json` (no trust) so the fixed mount has a source.
    /// The five guarantees (trust, bypass) are filled at the first claude spawn by
    /// `fill_staging_set`, which this test drives directly (as `spawn_node` does).
    #[test]
    fn ensure_ready_anchors_empty_json_and_the_first_claude_spawn_seeds_trust() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _log) = write_fake_docker(tmp.path());
        let ctx = test_ctx(tmp.path(), docker, full());

        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();

        // ensure_ready anchors a `.claude.json` for the fixed mount, but WITHOUT trust.
        let staged = staged_claude_json(&ctx.sandbox_root, "r1");
        assert!(staged.is_file(), "ensure_ready anchors a .claude.json");
        let key = ctx.repo_root.to_string_lossy().into_owned();
        let before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
        assert!(
            before["projects"][&key]["hasTrustDialogAccepted"].is_null(),
            "no trust before the first claude spawn: {before}"
        );

        // The first claude spawn fills the set: trust on repo_root + bypass permissions.
        staging::fill_staging_set(
            &ctx.home_root,
            &ctx.sandbox_root,
            "r1",
            pdo_harness::registry::CLAUDE,
            Some(ctx.repo_root.as_path()),
        )
        .unwrap();

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
        assert_eq!(
            after["projects"][&key]["hasTrustDialogAccepted"],
            serde_json::json!(true),
            "the claude spawn pre-approves the repo_root trust dialog: {after}"
        );
        let staged_settings = staged_claude_home(&ctx.sandbox_root, "r1").join("settings.json");
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged_settings).unwrap()).unwrap();
        assert_eq!(
            settings["skipDangerousModePermissionPrompt"],
            serde_json::json!(true),
            "the claude spawn holds the bypass fixup: {settings}"
        );
    }

    #[test]
    fn cleanup_removes_container_and_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let home_root = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        std::fs::create_dir_all(staged_claude_home(&sandbox_root, "r1")).unwrap();
        assert!(staging_dir_for_run(&sandbox_root, "r1").exists());

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
            !staging_dir_for_run(&sandbox_root, "r1").exists(),
            "cleanup purges the staging"
        );
    }

    #[test]
    fn kill_session_off_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        kill_session_best_effort(&docker, false, "r1", "pdo-r1-n1-iter-1");
        assert!(!log.exists(), "off must not invoke docker to kill");
    }

    #[test]
    fn kill_session_sandboxed_execs_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path());
        let logged = retry_side_effect(
            || kill_session_best_effort(&docker, true, "r1", "pdo-r1-n1-iter-1"),
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

    /// AC1 of #468, both halves. A profile WITH env poses `-e FOO=bar` at `docker create`;
    /// a profile WITHOUT env does not pose it — the negative control is mandatory, because
    /// asserting only the positive side would pass just as well against a fake that echoed
    /// every argument it was given.
    #[test]
    fn ensure_ready_poses_the_frozen_env_at_create_and_only_then() {
        // (a) with env → the `-e FOO=bar` reaches `docker create`.
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker_absent_container(tmp.path());
        let ctx = test_ctx_with(
            tmp.path(),
            docker,
            minimal(),
            Some(Vec::new()),
            env_map(&[("FOO", "bar")]),
        );
        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        let lines = log_lines(&log);
        assert!(lines.contains(&"create".to_string()), "absent → create");
        assert!(
            lines.contains(&"FOO=bar".to_string()),
            "the frozen env must be posed at create; log: {lines:?}"
        );

        // (b) NEGATIVE CONTROL: same harness, no env → no `FOO=` anywhere.
        let tmp2 = tempfile::tempdir().unwrap();
        let (docker2, log2) = write_fake_docker_absent_container(tmp2.path());
        let ctx2 = test_ctx_with(
            tmp2.path(),
            docker2,
            minimal(),
            Some(Vec::new()),
            BTreeMap::new(),
        );
        retry_etxtbsy(|| ensure_ready(&ctx2)).unwrap();
        let lines2 = log_lines(&log2);
        assert!(lines2.contains(&"create".to_string()), "absent → create");
        assert!(
            !lines2.iter().any(|l| l.starts_with("FOO=")),
            "a profile without env must pose nothing; log: {lines2:?}"
        );
    }

    /// AC1 + AC4 in one harness, at the layer where the two halves are comparable: the SAME
    /// `ensure_ready`, twice, differing only by the profile's image source.
    ///
    /// (a) an explicit ref reaches `docker create` as the image, with no build;
    /// (b) NEGATIVE CONTROL / AC4: a profile posing NOTHING produces the hash-derived tag and an
    ///     argv that is byte-identical to the pre-#467 one — asserted as full equality against the
    ///     other Run's argv, not by spot-checking, because "identical" is the whole claim.
    #[test]
    fn ensure_ready_uses_the_profiles_image_and_leaves_the_others_argv_untouched() {
        // (a) explicit ref → it IS the image at create, and nothing is built.
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker_absent_container(tmp.path());
        let ctx = test_ctx_full(
            tmp.path(),
            docker,
            minimal(),
            Some(Vec::new()),
            BTreeMap::new(),
            sandbox_image::ImagePlan::ExplicitRef {
                image_ref: "ghcr.io/acme/agent:1.4".to_string(),
                profile: "chrome".to_string(),
            },
        );
        retry_etxtbsy(|| ensure_ready(&ctx)).unwrap();
        let lines = log_lines(&log);
        assert!(lines.contains(&"create".to_string()), "absent → create");
        assert!(
            lines.contains(&"ghcr.io/acme/agent:1.4".to_string()),
            "the profile's ref must be the image at create; log: {lines:?}"
        );
        assert!(
            !lines.contains(&"build".to_string()),
            "an explicit ref has no Dockerfile to build; log: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("pdo-sandbox:h-")),
            "and the hash-derived tag must never appear; log: {lines:?}"
        );

        // (b) same harness, profile posing NOTHING → the historical hash-derived tag.
        let tmp2 = tempfile::tempdir().unwrap();
        let (docker2, log2) = write_fake_docker_absent_container(tmp2.path());
        let ctx2 = test_ctx_with(
            tmp2.path(),
            docker2,
            minimal(),
            Some(Vec::new()),
            BTreeMap::new(),
        );
        retry_etxtbsy(|| ensure_ready(&ctx2)).unwrap();
        let lines2 = log_lines(&log2);
        assert!(
            lines2.iter().any(|l| l.starts_with("pdo-sandbox:h-")),
            "no profile source ⇒ the content-addressed tag, as before #467; log: {lines2:?}"
        );
        assert!(
            !lines2.iter().any(|l| l.contains("ghcr.io/acme")),
            "and never another profile's ref; log: {lines2:?}"
        );
    }

    /// AC4 of #467, and the argv half of AC3 of #471: the `docker create` argv of a profile that
    /// poses no image source is **bit for bit** what it was, tempdir-relative paths aside. Proven
    /// by rebuilding the same context twice — once through a hand-written `HashDerived` plan over
    /// the `default` tier, once through `resolve_image_plan` with `profile_image: None` — and
    /// comparing the two argv logs verbatim.
    ///
    /// The two plans differ in their [`sandbox_image::ImageSource`] (the hand-written one builds,
    /// the resolved one pulls) and that is deliberate: the fake docker answers `image inspect`
    /// with exit 0, so BOTH take the fast path and land on the same content-addressed ref. What is
    /// compared is what `docker create` receives, which is exactly what must not move.
    #[test]
    fn a_profile_without_an_image_source_yields_the_same_create_argv() {
        let create_argv = |ctx: &Ctx, log: &Path| -> Vec<String> {
            retry_etxtbsy(|| ensure_ready(ctx)).unwrap();
            let lines = log_lines(log);
            let at = lines
                .iter()
                .position(|l| l == "create")
                .expect("the container is absent, so create must be reached");
            // Drop the tempdir-specific paths: what is compared is the SHAPE and the image.
            lines[at..]
                .iter()
                .map(|a| a.replace(ctx.sandbox_root.to_str().unwrap(), "<SB>"))
                .map(|a| a.replace(ctx.repo_root.to_str().unwrap(), "<REPO>"))
                .map(|a| a.replace(ctx.home_root.to_str().unwrap(), "<HOME>"))
                .map(|a| a.replace(ctx.pdo_bin.to_str().unwrap(), "<PDO>"))
                .collect()
        };

        // Reference: the plan the pre-#467 edge built (default tier, dockerfile mode).
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker_absent_container(tmp.path());
        let reference = test_ctx_with(
            tmp.path(),
            docker,
            minimal(),
            Some(Vec::new()),
            BTreeMap::new(),
        );
        let expected = create_argv(&reference, &log);

        // The #467 edge, with a profile that poses nothing: same plan, same argv.
        let tmp2 = tempfile::tempdir().unwrap();
        let (docker2, log2) = write_fake_docker_absent_container(tmp2.path());
        let sandbox_root2 = tmp2.path().join("sandbox");
        let through_plan = test_ctx_full(
            tmp2.path(),
            docker2,
            minimal(),
            Some(Vec::new()),
            BTreeMap::new(),
            // The PURE resolver (#471), env tiers explicitly empty: this test compares two argv
            // logs, so it must not depend on the environment of whoever runs it.
            sandbox_image::resolve_image_plan("minimal", None, None, None, &sandbox_root2),
        );
        assert_eq!(
            create_argv(&through_plan, &log2),
            expected,
            "a profile with no image source must not change one byte of `docker create`"
        );
    }

    #[test]
    fn cleanup_merges_transcripts_before_purging_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _log) = write_fake_docker(tmp.path());
        let home_root = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        let staged_projects = staged_claude_home(&sandbox_root, "r1").join("projects");
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

        assert!(
            home_root
                .join(".claude/projects/-enc-worktree/s.jsonl")
                .is_file(),
            "cleanup must merge transcripts to the host BEFORE teardown"
        );
        assert!(
            !staging_dir_for_run(&sandbox_root, "r1").exists(),
            "cleanup purges the staging after the merge"
        );
    }

    // -- Étage 2 : l'orchestration seule, ports en mémoire -----------------------

    use crate::application::testing::{InMemoryDockerfiles, InMemoryStaging, ScriptedDocker};

    fn in_memory(
        docker: ScriptedDocker,
    ) -> (SandboxRuntime, Arc<ScriptedDocker>, Arc<InMemoryStaging>) {
        let docker = Arc::new(docker);
        let staging = Arc::new(InMemoryStaging::default());
        let runtime = SandboxRuntime {
            docker: docker.clone(),
            staging: staging.clone(),
            dockerfiles: Arc::new(InMemoryDockerfiles::seeded(b"FROM scratch\n")),
        };
        (runtime, docker, staging)
    }

    fn memory_ctx(entries: Option<Vec<String>>) -> SandboxContext {
        let root = PathBuf::from("/sb");
        SandboxContext {
            run_id: "r1".into(),
            mode: minimal(),
            entries,
            env: BTreeMap::new(),
            repo_root: "/repo".into(),
            run_worktree: "/repo/.pdo/runs/r1/worktree".into(),
            daemon_port: 6172,
            home_root: "/home/u".into(),
            sandbox_root: root.clone(),
            host_home: "/home/u".into(),
            uid: 1000,
            gid: 1000,
            pdo_bin: "/bin/pdo".into(),
            image_plan: sandbox_image::ImagePlan::HashDerived {
                dockerfile: sandbox_image::resolve_dockerfile(None, None, &root),
                source: sandbox_image::ImageSource::Dockerfile,
            },
            writable_secondary_gitdirs: Vec::new(),
        }
    }

    #[test]
    fn the_first_prep_stages_then_images_then_starts_and_the_next_only_checks() {
        let (runtime, docker, staging) =
            in_memory(ScriptedDocker::answering(|args| match args[0].as_str() {
                "container" => ScriptedDocker::ok("true"),
                _ => ScriptedDocker::ok(""),
            }));
        let ctx = memory_ctx(Some(vec![".gitconfig".into()]));
        runtime.ensure_ready(&ctx).unwrap();
        runtime.ensure_ready(&ctx).unwrap();
        assert_eq!(
            staging.events(),
            vec!["prepare r1 [.gitconfig]", "anchors r1", "anchors r1"]
        );
        assert_eq!(
            docker.verbs(),
            vec!["image", "container", "exec", "image", "container", "exec"]
        );
    }

    #[test]
    fn an_off_run_touches_neither_docker_nor_the_staging() {
        let (runtime, docker, staging) = in_memory(ScriptedDocker::happy());
        runtime.ensure_ready(&memory_ctx(None)).unwrap();
        assert!(docker.calls().is_empty() && staging.events().is_empty());
    }

    #[test]
    fn a_failed_image_stops_before_the_container() {
        let (runtime, docker, _) =
            in_memory(ScriptedDocker::answering(|args| match args[0].as_str() {
                "image" => ScriptedDocker::fail(1, ""),
                "build" => ScriptedDocker::fail(1, "no space left"),
                _ => ScriptedDocker::ok(""),
            }));
        let err = runtime
            .ensure_ready(&memory_ctx(Some(Vec::new())))
            .unwrap_err();
        assert!(format!("{err:#}").contains("no space left"), "{err:#}");
        assert!(!docker.verbs().contains(&"container".to_string()));
    }

    #[test]
    fn cleanup_harvests_before_it_purges() {
        let (runtime, docker, staging) = in_memory(ScriptedDocker::happy());
        runtime.cleanup(Path::new("/home/u"), Path::new("/sb"), "r1");
        assert_eq!(staging.events(), vec!["merge_back r1", "teardown r1"]);
        assert_eq!(docker.verbs(), vec!["rm"]);
    }
}
