//! What the trigger context reads of the other contexts, behind its own ports:
//! the Runs a fire starts (`runs`), the repositories it names
//! (`repositories`), the pipelines it fires, the staging profiles its `sandbox`
//! names and the audit journal (here).

mod repositories;
mod runs;

use std::path::PathBuf;
use std::sync::Arc;

use pdo_pipeline::domain::pipeline::parse_pipeline;
use pdo_platform::domain::audit::NewAuditEntry;
use pdo_platform::Platform;
use pdo_run::application::paths::resolve_pipeline_path;
use pdo_sandbox::application::ports::SandboxProfileRepository;
use pdo_sandbox::domain::mode::SandboxMode;
use pdo_sandbox::domain::profile::{FULL_PROFILE, MINIMAL_PROFILE};

pub use repositories::GitRepositories;
pub use runs::RunsOfTriggers;

use crate::application::ports::{
    AuditTrail, BoxFuture, PipelineFacts, SandboxProfiles, TriggerPipelines,
};

/// The pipelines of the instance's library (falling back to the repo's own
/// `.pdo/pipelines`), read from disk at every call.
pub struct LibraryPipelines {
    pub repo_root: PathBuf,
}

impl TriggerPipelines for LibraryPipelines {
    fn resolve(&self, pipeline_id: &str) -> Option<Result<PipelineFacts, String>> {
        let yaml =
            std::fs::read_to_string(resolve_pipeline_path(&self.repo_root, pipeline_id)).ok()?;
        Some(
            parse_pipeline(&yaml)
                .map(|r| PipelineFacts {
                    name: r.pipeline.name.clone(),
                    prompt_required: r.pipeline.prompt_required,
                })
                .map_err(|e| e.to_string()),
        )
    }
}

/// The staging profiles of the sandbox context.
pub struct StagingProfiles(pub Arc<dyn SandboxProfileRepository>);

impl SandboxProfiles for StagingProfiles {
    /// The SHARED existence gate for any `sandbox` reference a client can store
    /// (ADR-0031 §7). `""` is the *clear* sentinel and `off` means "no sandbox", so
    /// both are always valid; anything else must name a profile that resolves.
    ///
    /// A **write-time** gate, deliberately not the authoritative one — the create-run
    /// chokepoint is, and it re-resolves at every fire. This one exists so the error
    /// lands where the user can fix it.
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(mode) = SandboxMode::parse(raw) else {
                return Ok(()); // "" / whitespace = the clear sentinel
            };
            let Some(name) = mode.profile() else {
                return Ok(()); // `off`
            };
            match pdo_sandbox::application::profiles::exists(self.0.as_ref(), name).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(format!(
                    "unknown sandbox profile `{name}`: use `off`, `{MINIMAL_PROFILE}`, \
                     `{FULL_PROFILE}`, or a profile you created under Settings → Staging \
                     profiles",
                )),
                Err(e) => Err(format!("sandbox profile store error: {e}")),
            }
        })
    }
}

/// The out-of-Run audit journal of the platform context.
pub struct PlatformAudit(pub Arc<Platform>);

impl AuditTrail for PlatformAudit {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.0.record_audit().execute(entry).await })
    }
}
