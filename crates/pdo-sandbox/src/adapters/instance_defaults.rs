//! The sandbox context's instance default, as `GET /settings` discloses it
//! (ADR-0015), and the facts the launch dialog reads beside it: the staging
//! profiles' names and whether Docker can run a sandbox at all. The stored
//! tier is handed in by the settings context, which keeps it.

use std::sync::Arc;

use pdo_kernel::instance_default::Source;
use serde::Serialize;

use super::env::{default_sandbox_with, env_default_sandbox};
use crate::application::docker_probe::{probe_cached, DockerProbeCache};
use crate::application::ports::{DockerCli, SandboxProfileRepository, StoreError};
use crate::application::profiles::{exists, names};
use crate::domain::mode::SandboxMode;

/// The instance default sandbox. The only sandbox knob of the settings page:
/// what a sandboxed Run *is* — image, home content, env — belongs to the
/// staging profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultSandbox {
    pub effective: String,
    pub source: Source,
    pub stored: Option<String>,
    pub env: Option<String>,
    pub default: &'static str,
    /// `null` when the winning tier resolves. The one knob whose value space is
    /// open (a profile name), hence the one that can point at nothing.
    pub reason: Option<String>,
}

/// `effective` is the resolver the create-run chokepoint consumes. The winning
/// tier may name a staging profile that does not exist, and every Run it
/// produces would die with a `400`: `PUT /settings` gates the stored tier, but
/// the env tier passes through no validator by construction, so the only
/// honest place to say so is here, as an advisory `reason`.
pub async fn default_sandbox(
    profiles: &dyn SandboxProfileRepository,
    stored: Option<&str>,
) -> DefaultSandbox {
    let stored_wins = stored.filter(|s| !s.is_empty()).map(str::to_string);
    let env = env_default_sandbox().map(|m| m.as_str().to_string());
    let effective = default_sandbox_with(stored.map(str::to_string));
    let source = Source::of(stored_wins.is_some(), env.is_some());
    let reason = match effective.profile() {
        None => None,
        Some(name) => match exists(profiles, name).await {
            Ok(true) => None,
            Ok(false) => Some(format!(
                "no staging profile named `{name}` — every Run that falls back to this \
                 default will fail at launch (tier: {})",
                source_name(source)
            )),
            Err(e) => Some(format!("cannot check staging profile `{name}`: {e}")),
        },
    };
    DefaultSandbox {
        effective: effective.as_str().to_string(),
        source,
        stored: stored_wins,
        env,
        default: SandboxMode::OFF_WIRE,
        reason,
    }
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Stored => "stored",
        Source::Env => "env",
        Source::Default => "default",
    }
}

/// A staging profile the launch dialog can pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileName {
    pub name: String,
    #[serde(rename = "virtual")]
    pub is_virtual: bool,
}

/// NAMES ONLY, on purpose: the launch dialog fetches the settings on every
/// open, and a `<select>` that needs three strings must not pay a per-profile
/// fold. The editor reads `GET /settings/sandbox-profiles` for the rest.
pub async fn profile_names(
    profiles: &dyn SandboxProfileRepository,
) -> Result<Vec<ProfileName>, StoreError> {
    Ok(names(profiles)
        .await?
        .into_iter()
        .map(|(name, is_virtual)| ProfileName { name, is_virtual })
        .collect())
}

/// Whether Docker can run a sandbox: an observed fact, not a knob. Advisory —
/// it grays out `full`/`minimal` in the dialog, but the run-advance fail-fast
/// (ADR-0030 pt 4) stays the authoritative gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerAvailability {
    pub available: bool,
    pub reason: Option<String>,
    pub checked_at: String,
}

/// The cached probe (refreshed when stale, never on the runtime thread).
pub async fn docker_availability(
    cache: &DockerProbeCache,
    docker: Arc<dyn DockerCli>,
) -> DockerAvailability {
    let probe = probe_cached(cache, docker).await;
    DockerAvailability {
        available: probe.available,
        reason: probe.reason,
        checked_at: pdo_kernel::clock::now_iso(),
    }
}
