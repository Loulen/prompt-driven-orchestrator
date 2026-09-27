//! The [`SandboxContext`] of a Run: everything preparing its sandbox needs, resolved
//! once from the Run's **frozen** profile choice (ADR-0031 §6) and the host facts
//! the caller hands in.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;

use super::ports::SandboxProfileRepository;
use crate::domain::frozen::{
    frozen_entries, frozen_env, frozen_image, FrozenEntries, FrozenSandbox,
};
use crate::domain::image::{resolve_image_plan, ImagePlan, ImageSource};
use crate::domain::mode::SandboxMode;
use crate::domain::profile::resolve_profile;

/// Everything [`super::prepare::SandboxRuntime::ensure_ready`] needs, as owned values,
/// so the preparation never reaches back into the Run.
#[derive(Debug, Clone)]
pub struct SandboxContext {
    pub run_id: String,
    /// `off`, or the staging profile this Run was launched with. The ONE place in the
    /// tree that still needs the profile *name* rather than its off-ness (#432 D2).
    pub mode: SandboxMode,
    /// The profile's entry list as **frozen at creation** (ADR-0031 §6). `None` for
    /// `off`. `Some(vec![])` is a legitimate resolution — it is `minimal`.
    pub entries: Option<Vec<String>>,
    /// The profile's env as **frozen at creation** (#468, ADR-0031 §8). Empty for `off`
    /// and for every profile that declares none — an empty map and an absent payload key
    /// mean the same container, which is why this is a plain `BTreeMap` and not the
    /// `Option` that `entries` needs.
    pub env: BTreeMap<String, String>,
    /// Effective repo root — bind-mounted rw at its host path. One mount covers the repo
    /// + every node sub-worktree under `.pdo/runs/` + `.pdo/prompts`.
    pub repo_root: PathBuf,
    /// The Run's pipeline worktree (`-w` cosmetic at create; the trust dialog is seeded
    /// by the staging set's trust fixup on `repo_root`, the common ancestor of every
    /// worktree — #426).
    pub run_worktree: PathBuf,
    pub daemon_port: u16,
    /// Host `$HOME` — source of `.claude` for the staging.
    pub home_root: PathBuf,
    /// `$HOME/.pdo/sandbox` — per-Run staging lives under `<sandbox_root>/<run>`.
    pub sandbox_root: PathBuf,
    /// Host `$HOME` again, as the mount-target root for `.claude`/`.claude.json` inside
    /// the container (kept distinct from `home_root` to mirror the two staging params
    /// it feeds, even though both resolve to `$HOME`).
    pub host_home: PathBuf,
    pub uid: u32,
    pub gid: u32,
    /// Host `pdo` binary, bind-mounted read-only at `/usr/local/bin/pdo`.
    pub pdo_bin: PathBuf,
    /// WHERE this Run's image comes from, resolved ONCE (#467 the profile's own source,
    /// else the profile default of #471): hash-derived (a Dockerfile + pull/build) or an
    /// explicit registry ref (pull or fail) — see [`ImagePlan`].
    pub image_plan: ImagePlan,
    /// Host `.git` directories of the Run's **writable** secondary repos (ADR-0047),
    /// each bind-mounted rw at its identical host path so `git` works inside a writable
    /// secondary snapshot. Empty for a mono-repo Run, an all-read-only one, or host mode.
    /// Frozen at container creation, like every other mount.
    pub writable_secondary_gitdirs: Vec<PathBuf>,
}

/// What the Run side knows about a Run when it asks for its sandbox context: its
/// frozen choice, its paths, and the host facts read at the edge.
#[derive(Debug, Clone)]
pub struct RunSandbox {
    pub mode: SandboxMode,
    pub frozen: FrozenSandbox,
    pub repo_root: PathBuf,
    pub run_worktree: PathBuf,
    pub daemon_port: u16,
    pub home_root: PathBuf,
    pub sandbox_root: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub pdo_bin: PathBuf,
    pub writable_secondary_gitdirs: Vec<PathBuf>,
    /// The two env tiers of the profile default image (#471), read once at the edge.
    pub env_image_source: Option<ImageSource>,
    pub env_dockerfile: Option<String>,
}

/// Resolve the entry list the Run froze, re-resolving a virtual default only when the
/// payload predates the freeze ([`frozen_entries`]'s table).
pub async fn resolve_frozen_entries(
    profiles: &dyn SandboxProfileRepository,
    frozen: &FrozenSandbox,
    name: &str,
) -> Result<Vec<String>> {
    match frozen_entries(frozen, name)? {
        FrozenEntries::Frozen(entries) => Ok(entries),
        FrozenEntries::ReResolveVirtual => {
            info!(
                "run {} predates the frozen sandbox entry list; re-resolving the built-in \
                 default `{name}`",
                frozen.run_id
            );
            let stored = profiles
                .get(name)
                .await
                .with_context(|| format!("resolve the built-in staging profile `{name}`"))?;
            let resolved = resolve_profile(name, stored).ok_or_else(|| {
                anyhow::anyhow!("built-in staging profile `{name}` did not resolve")
            })?;
            Ok(resolved.resolved.entries)
        }
    }
}

/// Assemble the [`SandboxContext`] of a Run.
///
/// Fails (loud) when the Run's **frozen** staging-profile selection cannot be resolved
/// (#432) — a sandboxed Run must never fall back to a half-configured container, or to
/// a *different* home content, silently.
pub async fn assemble_context(
    profiles: &dyn SandboxProfileRepository,
    run: RunSandbox,
) -> Result<SandboxContext> {
    // #432: the frozen entry list. `off` resolves to `None` without touching the store.
    let entries = match run.mode.profile() {
        None => None,
        Some(name) => Some(resolve_frozen_entries(profiles, &run.frozen, name).await?),
    };
    // #468: the frozen env, with the same "loud on unreadable" rule. `off` never reaches
    // it — an empty map is what `ensure_ready` would ignore anyway.
    let env = if entries.is_some() {
        frozen_env(&run.frozen)?
    } else {
        BTreeMap::new()
    };
    // #467: the image plan, from the profile's FROZEN choice folded over the profile
    // default (#471). `off` never reaches `ensure_ready`, so the plan it gets is the
    // default one — inert, and cheaper than an `Option` every consumer would unwrap.
    let profile_image = if entries.is_some() {
        frozen_image(&run.frozen)?
    } else {
        None
    };
    let image_plan = resolve_image_plan(
        // `as_str()` and not `profile()`: the label only ever surfaces in the
        // explicit-ref failure reason — unreachable for `off`, since posing a ref
        // requires a profile.
        run.mode.as_str(),
        profile_image.as_ref(),
        run.env_image_source,
        run.env_dockerfile.as_deref(),
        &run.sandbox_root,
    );
    Ok(SandboxContext {
        run_id: run.frozen.run_id,
        mode: run.mode,
        entries,
        env,
        repo_root: run.repo_root,
        run_worktree: run.run_worktree,
        daemon_port: run.daemon_port,
        host_home: run.home_root.clone(),
        home_root: run.home_root,
        sandbox_root: run.sandbox_root,
        uid: run.uid,
        gid: run.gid,
        pdo_bin: run.pdo_bin,
        image_plan,
        writable_secondary_gitdirs: run.writable_secondary_gitdirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::InMemoryProfiles;
    use crate::domain::image::ProfileImage;
    use crate::domain::profile::ProfileDiff;

    fn run(mode: SandboxMode, frozen: FrozenSandbox) -> RunSandbox {
        RunSandbox {
            mode,
            frozen,
            repo_root: "/repo".into(),
            run_worktree: "/repo/.pdo/runs/r1/worktree".into(),
            daemon_port: 6172,
            home_root: "/home/u".into(),
            sandbox_root: "/home/u/.pdo/sandbox".into(),
            uid: 1000,
            gid: 1000,
            pdo_bin: "/bin/pdo".into(),
            writable_secondary_gitdirs: Vec::new(),
            env_image_source: None,
            env_dockerfile: None,
        }
    }

    fn frozen(entries: Option<Vec<String>>) -> FrozenSandbox {
        FrozenSandbox {
            run_id: "r1".into(),
            entries,
            ..FrozenSandbox::default()
        }
    }

    #[tokio::test]
    async fn off_touches_nothing_and_carries_no_entries() {
        let profiles = InMemoryProfiles::default();
        let ctx = assemble_context(&profiles, run(SandboxMode::Off, frozen(None)))
            .await
            .unwrap();
        assert_eq!(ctx.entries, None);
        assert!(ctx.env.is_empty());
        assert_eq!(ctx.host_home, ctx.home_root);
    }

    #[tokio::test]
    async fn the_frozen_choice_wins_over_the_live_profile() {
        let profiles = InMemoryProfiles::default();
        profiles.seed(ProfileDiff {
            name: "team".into(),
            disabled: Vec::new(),
            extras: vec![".npmrc".into()],
            env: BTreeMap::from([("LIVE".to_string(), "1".to_string())]),
            image: None,
            updated_at: "t".into(),
        });
        let mut f = frozen(Some(vec![".gitconfig".into()]));
        f.env = Some(BTreeMap::from([("FROZEN".to_string(), "1".to_string())]));
        f.image = Some(ProfileImage::Registry {
            image_ref: "acme/agent:1".into(),
        });
        let ctx = assemble_context(&profiles, run(SandboxMode::Profile("team".into()), f))
            .await
            .unwrap();
        assert_eq!(ctx.entries, Some(vec![".gitconfig".to_string()]));
        assert!(ctx.env.contains_key("FROZEN") && !ctx.env.contains_key("LIVE"));
        assert_eq!(ctx.image_plan.known_ref(), Some("acme/agent:1"));
    }

    #[tokio::test]
    async fn a_pre_freeze_virtual_default_is_re_resolved_from_the_store() {
        let profiles = InMemoryProfiles::default();
        profiles.seed(ProfileDiff {
            name: "minimal".into(),
            disabled: Vec::new(),
            extras: vec![".gitconfig".into()],
            env: BTreeMap::new(),
            image: None,
            updated_at: "t".into(),
        });
        let ctx = assemble_context(
            &profiles,
            run(SandboxMode::Profile("minimal".into()), frozen(None)),
        )
        .await
        .unwrap();
        assert_eq!(ctx.entries, Some(vec![".gitconfig".to_string()]));
    }
}
