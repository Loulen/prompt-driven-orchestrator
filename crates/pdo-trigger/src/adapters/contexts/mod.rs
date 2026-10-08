//! What the trigger context reads of the other contexts, behind its own ports:
//! the Runs a fire starts (`runs`), the repositories it names
//! (`repositories`), the pipelines it fires, the staging profiles its `sandbox`
//! names and the audit journal (here); and the Triggers as the run context
//! reads them (`pipeline_triggers`).

mod pipeline_triggers;
mod repositories;
mod runs;

use std::path::PathBuf;
use std::sync::Arc;

use pdo_pipeline::domain::pipeline::parse_pipeline;
use pdo_platform::domain::audit::NewAuditEntry;
use pdo_platform::Platform;
use pdo_run::application::paths::resolve_pipeline_path;
use pdo_sandbox::application::ports::SandboxProfileRepository;

pub use pipeline_triggers::TriggersOfRuns;
pub use repositories::GitRepositories;
pub use runs::{ProjectsOfRuns, RunsOfTriggers};

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
    /// The sandbox context's write-time gate.
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(pdo_sandbox::application::profiles::validate_ref(
            self.0.as_ref(),
            raw,
        ))
    }
}

/// The out-of-Run audit journal of the platform context.
pub struct PlatformAudit(pub Arc<Platform>);

impl AuditTrail for PlatformAudit {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.0.record_audit().execute(entry).await })
    }
}
