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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::event_log::{RunState, SandboxMode};
use crate::{sandbox_container, sandbox_image, sandbox_staging, AppState};

/// Everything [`ensure_ready`] / [`cleanup`] need, assembled once at the boundary
/// by [`context_from_state`] from `AppState` + the projected `RunState`. Holds
/// owned values so the core never reaches back into `AppState`.
pub(crate) struct SandboxContext {
    /// The `docker` binary to invoke (`state.docker_cmd_override` → `"docker"`).
    pub(crate) docker_bin: String,
    pub(crate) run_id: String,
    /// `off`, or the staging profile this Run was launched with. The ONE place in the
    /// tree that still needs the profile *name* rather than its off-ness — hence the
    /// single `.clone()` of [`context_from_state`] (#432 D2).
    pub(crate) mode: SandboxMode,
    /// The profile's entry list as **frozen at creation** (ADR-0031 §6), resolved at
    /// the boundary. `None` for `off`. `Some(vec![])` is a legitimate resolution — it
    /// is `minimal`.
    pub(crate) entries: Option<Vec<String>>,
    /// The profile's env as **frozen at creation** (#468, ADR-0031 §8), resolved at the
    /// boundary by [`frozen_env`]. Empty for `off` and for every profile that declares
    /// none — an empty map and an absent payload key mean the same container, which is why
    /// this is a plain `BTreeMap` and not the `Option` that `entries` needs.
    pub(crate) env: BTreeMap<String, String>,
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
    /// WHERE this Run's image comes from, resolved ONCE at the edge (#467 the profile's own
    /// source, else the profile default of #471). Two shapes in one value —
    /// hash-derived (a Dockerfile + pull/build) or an explicit registry ref (pull or fail) —
    /// because the two are not variations of one procedure: see
    /// [`sandbox_image::ImagePlan`]. Passed to `sandbox_image::ensure_image` in the sync
    /// path, so the core still needs no DB access.
    pub(crate) image_plan: sandbox_image::ImagePlan,
    /// Host `.git` directories of the Run's **writable** secondary repos
    /// (ADR-0047). Each is bind-mounted rw at its identical host path so `git`
    /// works inside a writable secondary snapshot (whose gitdir lives under
    /// `<secondary>/.git`, outside `repo_root` and thus otherwise unmounted).
    /// Empty for a mono-repo Run, an all-read-only one, or host mode. Frozen at
    /// container creation, like every other mount (ADR-0047 conséquence 2).
    pub(crate) writable_secondary_gitdirs: Vec<PathBuf>,
}

/// Resolve the entry list a sandboxed Run must stage, from its **frozen** projection
/// (#432, ADR-0031 §6). The decision table, and why each arm is what it is:
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | name + valid entries | **entries verbatim**, the name is ignored | the pure form of the freeze. A profile deleted since is *not* an error — surviving that is the point |
/// | `full`/`minimal`, **no** entries | resolve the virtual default now | a virtual default always resolves; `RunFailed` on a perfectly resolvable Run is indefensible |
/// | a user profile, **no** entries | **hard error** → `RunFailed` | unreachable by construction (the one chokepoint writes both keys) |
/// | entries present but unreadable | **hard error**, raw value in the reason | undecidable; any fallback silently changes what the already-spawned nodes saw |
///
/// The `full`/`minimal`-without-entries arm does, in effect, read the **live** setting.
/// Assumed and documented (ADR-0031 §6 note): it only fires for a Run created by a
/// pre-profiles daemon, cleaned up, then resumed. Both alternatives are worse
/// (`RunFailed` on something resolvable; or freezing the #426 default into Rust for
/// ever, which contradicts ADR-0031 §2).
async fn frozen_entries(
    db: &sqlx::SqlitePool,
    run_state: &RunState,
    name: &str,
) -> Result<Vec<String>> {
    if let Some(raw) = &run_state.sandbox_entries_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox entry list ({raw}); refusing to re-resolve \
             a different list for a Run whose nodes already staged one",
            run_state.run_id
        );
    }
    if let Some(entries) = &run_state.sandbox_entries {
        // Frozen list wins outright — even if the profile has been edited or deleted.
        return Ok(entries.clone());
    }
    // No frozen list: a pre-#432 payload. Only a virtual default may be re-resolved.
    if !crate::sandbox_profile::VIRTUAL_PROFILES.contains(&name) {
        anyhow::bail!(
            "run {} names the staging profile `{name}` but froze no entry list \
             (payload predates #432 and `{name}` is not a built-in default); refusing to \
             guess what it should stage",
            run_state.run_id
        );
    }
    info!(
        "run {} predates the frozen sandbox entry list; re-resolving the built-in \
         default `{name}`",
        run_state.run_id
    );
    let resolved = crate::sandbox_profile::resolve(db, name)
        .await
        .with_context(|| format!("resolve the built-in staging profile `{name}`"))?
        .ok_or_else(|| anyhow::anyhow!("built-in staging profile `{name}` did not resolve"))?;
    Ok(resolved.resolved.entries)
}

/// Resolve the env a sandboxed Run must pose at `docker create`, from its **frozen**
/// projection (#468, ADR-0031 §8). Pure — no DB, unlike [`frozen_entries`].
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | `sandbox_env` present and readable | **verbatim** | the freeze. A profile edited (or deleted) since is not an error — surviving that is the point |
/// | absent | **empty** | a pre-#468 daemon could pose no profile env at all, so absence and emptiness describe the same container. Reading the live profile here would *add* variables to a Run in flight — the exact retroactivity §6 forbids |
/// | present but unreadable | **hard error** | undecidable; posing "no env" instead would start the container without the variables its MCP servers need and look like a plugin bug |
///
/// The `frozen_entries` twin has a fourth row — re-resolving a *virtual default* whose list
/// is absent — and this one deliberately has not. There, the alternative was `RunFailed` on
/// a perfectly resolvable Run; here, "no env" is not a guess, it is what the Run actually
/// ran with.
///
/// The error message names the raw value, like `frozen_entries` does: this arm can only fire
/// on a payload that is not a map of strings, i.e. one that cannot be carrying the user's
/// values in the first place.
fn frozen_env(run_state: &RunState) -> Result<BTreeMap<String, String>> {
    if let Some(raw) = &run_state.sandbox_env_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox env ({raw}); refusing to start a container \
             with a different environment than the Run's nodes already saw",
            run_state.run_id
        );
    }
    Ok(run_state.sandbox_env.clone().unwrap_or_default())
}

/// Resolve the image source a sandboxed Run's **profile** froze at creation (#467, ADR-0031 §9).
/// Pure, and a strict copy of [`frozen_env`]'s three rows — same decision table, same reasons:
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | `sandbox_image` present and readable | **verbatim** | the freeze. A profile whose image was edited (or deleted) since is not an error — surviving that is the point |
/// | absent | **`None`** = "the profile posed none" | a pre-#467 daemon could pose none at all, so absence and "poses none" describe the same resolution: the profile default decides (`sandbox_profile::DEFAULT_PROFILE_IMAGE`, #471), exactly as before |
/// | present but unreadable | **hard error** | undecidable; falling back to the profile default would start the container in a DIFFERENT image than the nodes that already launched ran in — the one failure this freeze exists to prevent |
///
/// Note what `None` does **not** mean: it is not "no image". Every sandboxed Run has an image;
/// `None` only says the profile did not choose it.
fn frozen_image(run_state: &RunState) -> Result<Option<sandbox_image::ProfileImage>> {
    if let Some(raw) = &run_state.sandbox_image_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox image source ({raw}); refusing to start a \
             container from a different image than the Run's nodes already ran in",
            run_state.run_id
        );
    }
    Ok(run_state.sandbox_image.clone())
}

/// Resolve a [`SandboxContext`] from the daemon state + a projected Run. The one
/// edge function that reads `AppState` / the environment; the core is pure values.
///
/// **Async**: reads the DB at the boundary to resolve the Run's **frozen** staging profile
/// (#432 the entry list, #468 the env). Since #471 the image plan needs no DB read at all —
/// the two instance-wide knobs it used to fold in are gone, and what is left is the Run's frozen
/// profile choice folded over a compile-time default plus two env vars.
///
/// Fails (loud) when `$HOME` is unset, the current exe path can't be resolved, or the
/// Run's **frozen** staging-profile selection cannot be resolved (#432) — a sandboxed
/// Run must never fall back to a half-configured container, or to a *different* home
/// content, silently. The one caller that used to swallow this error (boot recovery)
/// now turns it into an explicit `RunFailed`.
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
    // #432: the frozen entry list, resolved here (the boundary) so the sync core stays
    // DB-free. `off` resolves to `None` without touching the DB.
    let entries = match run_state.sandbox.profile() {
        None => None,
        Some(name) => Some(frozen_entries(&state.db, run_state, name).await?),
    };
    // #468: the frozen env, on the same boundary and with the same "loud on unreadable"
    // rule. `off` never reaches it — an empty map is what `ensure_ready` would ignore anyway.
    let env = if entries.is_some() {
        frozen_env(run_state)?
    } else {
        BTreeMap::new()
    };
    // #467: the image plan, on the same boundary, from the profile's FROZEN choice folded over
    // the profile default (#471). `off` never reaches `ensure_ready`, so the plan it gets is the
    // default one — inert, and cheaper than an `Option` every consumer would have to unwrap.
    let image_plan = sandbox_image::image_plan_with(
        // `as_str()` and not `profile()`: it yields the profile name when there is one and `off`
        // otherwise, and the label only ever surfaces in the explicit-ref failure reason — which
        // is unreachable for `off`, since posing a ref requires a profile.
        run_state.sandbox.as_str(),
        if entries.is_some() {
            frozen_image(run_state)?
        } else {
            None
        }
        .as_ref(),
        &sandbox_root,
    );
    // ADR-0047: harvest the `.git` of every WRITABLE secondary (read_only ==
    // false). A writable secondary snapshot is a detached worktree whose object
    // store is `<secondary>/.git` — outside `repo_root`, so the single repo mount
    // does not cover it. Bind-mounting that gitdir rw (below, at container
    // creation) is what makes `git status`/`commit` work inside it. Read-only
    // secondaries get nothing.
    let writable_secondary_gitdirs = run_state
        .target_repos
        .iter()
        .filter(|pin| !pin.read_only)
        .map(|pin| Path::new(&pin.repo).join(".git"))
        .collect();
    Ok(SandboxContext {
        docker_bin: docker_bin(state),
        run_id: run_state.run_id.clone(),
        // The ONE clone of the tree (#432 D2): every other consumer only tests
        // off-ness and takes a `bool`.
        mode: run_state.sandbox.clone(),
        entries,
        env,
        repo_root,
        run_worktree,
        daemon_port: state.port,
        home_root,
        sandbox_root,
        host_home,
        uid: sandbox_container::host_uid(),
        gid: sandbox_container::host_gid(),
        pdo_bin,
        image_plan,
        writable_secondary_gitdirs,
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
/// `sandboxed` is a plain `bool`, not a [`SandboxMode`] (#432 D2): this seam only ever
/// asked about off-ness, and since the mode owns a `String` a by-value parameter would
/// force a clone at each of the call sites — several of which capture it into a detached
/// `async move`, where a reference cannot live.
pub(crate) fn transcripts_root(
    sandboxed: bool,
    run_id: &str,
    home_root: &Path,
    sandbox_root: &Path,
) -> PathBuf {
    if sandboxed && sandbox_staging::staging_dir_for_run(sandbox_root, run_id).exists() {
        sandbox_staging::staged_claude_home(sandbox_root, run_id).join("projects")
    } else {
        home_root.join(".claude").join("projects")
    }
}

/// The `copilot` session-state store root — `<home_root>/.copilot/session-state/`,
/// where each session's event journal lives at `<session-id>/events.jsonl` (#615).
///
/// Always the **host** home, unlike [`transcripts_root`]: `copilot` declares **no
/// staging floor** (ADR-0031 / #615), so a sandboxed Run has no staged copilot home
/// to mirror — the journal is read where the harness wrote it. Path math only; this
/// module never reads `$HOME` (the caller injects `home_root`).
pub(crate) fn copilot_store_root(home_root: &Path) -> PathBuf {
    home_root.join(".copilot").join("session-state")
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
    let Some(entries) = ctx.entries.as_deref() else {
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
        // Name the profile AND the list once per staging, so "why does this Run carry
        // plugins/" is answerable from the log rather than by reading the DB (which may
        // have been edited since — the list is frozen, the profile is not).
        info!(
            "sandbox: staging run {} with profile `{}` ({} entries: {})",
            ctx.run_id,
            ctx.mode.as_str(),
            entries.len(),
            entries.join(", ")
        );
        // #468: the env gets the SAME once-per-staging visibility and a DIFFERENT rule —
        // the **names**, never the values. The line above lists staging entries in clear
        // because a `$HOME`-relative path is not a secret; an env value routinely is (a
        // client endpoint, a proxy credential, an API token someone put here despite the
        // UI saying this is not a secret store). The systemd journal outlives the Run, so
        // a leak there is an incident that cannot be undone by deleting the profile.
        // `env_names` is a named function precisely so that rule has a unit test.
        if !ctx.env.is_empty() {
            info!(
                "sandbox: run {} carries {} profile env var(s) (names only: {})",
                ctx.run_id,
                ctx.env.len(),
                crate::sandbox_profile::env_names(&ctx.env)
            );
        }
        // `Some(repo_root)` unconditionally: the `off` arm of the old match was DEAD —
        // the `let Some(entries) = … else { return }` above has already excluded it.
        sandbox_staging::prepare(
            &ctx.home_root,
            &ctx.sandbox_root,
            entries,
            &ctx.run_id,
            Some(ctx.repo_root.as_path()),
        )
        .with_context(|| format!("failed to stage the sandbox home for run {}", ctx.run_id))?;
    }

    // 2. Ensure the Run's image exists locally, per the plan resolved at the edge:
    //    pull-then-retag `<name>:h-<hash>` from GHCR (registry, default) or build it from the
    //    RESOLVED Dockerfile (dockerfile mode / custom path / pull fallback), per
    //    the [`ImageSource`] and the winning `dockerfile` tier (#431, #467, #471) — OR pull a
    //    profile's EXPLICIT registry ref, where a failed pull is a hard error and no build is
    //    attempted (#467, ADR-0030 pt 7 as amended). A resolved Dockerfile path that is not a
    //    readable regular file is likewise a hard error, never a silent fallback to the seeded
    //    default (ADR-0030 pt 4).
    let image_ref =
        sandbox_image::ensure_image(&ctx.docker_bin, &ctx.sandbox_root, &ctx.image_plan)
            .context("failed to ensure the sandbox image")?;
    // Name the image ONCE per prep (not per node — `ensure_ready` is not on the spawn path), so
    // "which image did this Run actually get" is answerable from the log. It cannot be answered
    // from the profile afterwards: the source is frozen, the profile is not. An image ref is not
    // a secret (unlike the env values two blocks up), so it goes in clear.
    info!(
        "sandbox: run {} uses image {image_ref} (source: {})",
        ctx.run_id,
        match &ctx.image_plan {
            sandbox_image::ImagePlan::ExplicitRef { profile, .. } =>
                format!("explicit ref from profile `{profile}`"),
            sandbox_image::ImagePlan::HashDerived { dockerfile, .. } => format!(
                "content hash of {} (`{}` tier)",
                dockerfile.path.display(),
                dockerfile.source.as_str()
            ),
        }
    );

    // 3. Assemble the container spec + ensure the long-lived container is up.
    let staged_home = sandbox_staging::staged_claude_home(&ctx.sandbox_root, &ctx.run_id);
    let staged_json = sandbox_staging::staged_claude_json(&ctx.sandbox_root, &ctx.run_id);
    // #432: derived from `frozen list × disk`, NOT returned by `prepare` — `prepare` is
    // skipped whenever the staging already exists (3 of the 4 `ensure_ready` callers),
    // and this derivation gives the same answer either way.
    let extra =
        sandbox_staging::extra_mounts(&ctx.sandbox_root, &ctx.run_id, &ctx.host_home, entries);
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
        extra_mounts: &extra,
        // #468: the FROZEN env. `ensure_running` only consults the spec on its `Absent`
        // arm — `docker start` never re-evaluates a pre-existing container's env, any more
        // than its mounts. That is exactly the freeze of ADR-0031 §6/§8, guaranteed twice.
        env: &ctx.env,
        // ADR-0047: the `.git` of each writable secondary, mounted rw so `git`
        // works inside it. Same freeze caveat as the mounts above — a secondary
        // added writable mid-run only sees its gitdir mounted after container
        // recreation (documented limitation, ADR-0047 conséquence 2).
        writable_secondary_gitdirs: &ctx.writable_secondary_gitdirs,
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
    sandboxed: bool,
    run_id: &str,
    marker: &str,
) {
    if !sandboxed {
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
        SandboxMode::Profile(crate::sandbox_profile::FULL_PROFILE.into())
    }
    fn minimal() -> SandboxMode {
        SandboxMode::Profile(crate::sandbox_profile::MINIMAL_PROFILE.into())
    }

    /// Build a context rooted under temp dirs (bypasses the env/exe resolvers).
    ///
    /// #432: `mode` names the profile and `entries` is the FROZEN list — the two are
    /// separate parameters here because the point of the freeze is that they *can*
    /// disagree with whatever the store now says. `Off` carries `None`.
    fn test_ctx(tmp: &Path, docker_bin: String, mode: SandboxMode) -> SandboxContext {
        let entries = match mode.profile() {
            None => None,
            Some(name) => {
                let base = crate::sandbox_profile::base_entries(name);
                Some(crate::sandbox_profile::resolve_entry_list(&base, &[], &[]).entries)
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
    ) -> SandboxContext {
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
    ) -> SandboxContext {
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
    ) -> SandboxContext {
        let home = tmp.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
        let sandbox_root = tmp.join("sandbox");
        SandboxContext {
            docker_bin,
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
        let ctx = test_ctx(tmp.path(), docker, minimal());

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
        let ctx = test_ctx(tmp.path(), docker, full());

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

    // --- #468: the frozen profile env ------------------------------------------

    fn run_state_with_env(env: Option<BTreeMap<String, String>>) -> RunState {
        let mut rs = RunState::new("r1".to_string(), "p".to_string());
        rs.sandbox = full();
        rs.sandbox_entries = Some(Vec::new());
        rs.sandbox_env = env;
        rs
    }

    /// Row 1 of the decision table: a frozen env is used **verbatim**, whatever the store
    /// now says. That is what makes an edit non-retroactive.
    #[test]
    fn frozen_env_is_used_verbatim() {
        let rs = run_state_with_env(Some(env_map(&[("FOO", "bar")])));
        assert_eq!(frozen_env(&rs).unwrap(), env_map(&[("FOO", "bar")]));
    }

    /// Row 2, and the asymmetry with [`frozen_entries`]: an ABSENT env is not a legacy arm
    /// to re-resolve, it is "no env". Re-resolving would add variables to a Run in flight.
    #[test]
    fn an_absent_frozen_env_is_empty_not_re_resolved() {
        assert!(frozen_env(&run_state_with_env(None)).unwrap().is_empty());
        // An explicitly frozen empty map is the same answer — by construction.
        assert!(frozen_env(&run_state_with_env(Some(BTreeMap::new())))
            .unwrap()
            .is_empty());
    }

    /// Row 3: unreadable is a HARD error naming the raw value, never a silent "no env" —
    /// which would start the container without the variables its MCP servers need and look
    /// like a plugin bug.
    #[test]
    fn an_unreadable_frozen_env_fails_loud() {
        let mut rs = run_state_with_env(None);
        rs.sandbox_env_raw_error = Some("42".to_string());
        let err = frozen_env(&rs).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("42"),
            "the reason must name the raw value: {msg}"
        );
        assert!(msg.contains("unreadable"), "{msg}");
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

    // --- #467: the frozen profile image source ---------------------------------

    fn run_state_with_image(image: Option<sandbox_image::ProfileImage>) -> RunState {
        let mut rs = RunState::new("r1".to_string(), "p".to_string());
        rs.sandbox = full();
        rs.sandbox_entries = Some(Vec::new());
        rs.sandbox_image = image;
        rs
    }

    /// Row 1: a frozen image source is used **verbatim**, whatever the store now says. That is
    /// what makes an edit non-retroactive — the AC2 guarantee, at the unit level.
    #[test]
    fn frozen_image_is_used_verbatim() {
        let img = sandbox_image::ProfileImage::Registry {
            image_ref: "ghcr.io/acme/agent:1.4".to_string(),
        };
        assert_eq!(
            frozen_image(&run_state_with_image(Some(img.clone()))).unwrap(),
            Some(img)
        );
    }

    /// Row 2: an ABSENT source means "the profile posed none" — NOT "no image". The instance-wide
    /// setting then decides, read fresh, exactly as before #467.
    #[test]
    fn an_absent_frozen_image_is_none_not_an_error() {
        assert_eq!(frozen_image(&run_state_with_image(None)).unwrap(), None);
    }

    /// Row 3: unreadable is a HARD error naming the raw value, never a silent fallback to the
    /// instance setting — which would start the container in a DIFFERENT image than the nodes that
    /// already launched ran in.
    #[test]
    fn an_unreadable_frozen_image_fails_loud() {
        let mut rs = run_state_with_image(None);
        rs.sandbox_image_raw_error = Some("{\"kind\":\"ecr\"}".to_string());
        let err = frozen_image(&rs).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("ecr"),
            "the reason must name the raw value: {msg}"
        );
        assert!(msg.contains("unreadable"), "{msg}");
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
        let create_argv = |ctx: &SandboxContext, log: &Path| -> Vec<String> {
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
            transcripts_root(false, "r1", &home, &sandbox_root),
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
            transcripts_root(true, "r1", &home, &sandbox_root),
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
            transcripts_root(true, "r1", &home, &sandbox_root),
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
        let host = transcripts_root(true, "r1", &home, &sandbox_root);
        assert_eq!(host.join(&enc), home.join(".claude/projects").join(&enc));

        // Staging arm (staging materialised) — same encoded segment appended.
        std::fs::create_dir_all(sandbox_staging::staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        let staged = transcripts_root(true, "r1", &home, &sandbox_root);
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
