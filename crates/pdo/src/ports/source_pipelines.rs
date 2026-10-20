//! The shared pipelines a Run's snapshot overwrites: the run context's
//! `SourcePipelines` port, over the pipeline context's registry (wired after
//! the Runs, ADR-0085), and the version a new Run launches (ADR-0088).

use std::collections::HashMap;

use pdo_kernel::Actor;
use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::version::PipelineVersionRef;
use pdo_pipeline::Pipelines;
use pdo_run::application::ports::{BoxFuture, OverwriteRefusal, SourcePipelines};

use super::Late;

/// The registry of the pipeline context.
pub(crate) struct RegistryPipelines(pub(crate) Late<Pipelines>);

impl SourcePipelines for RegistryPipelines {
    fn exists(&self, id: &str) -> bool {
        self.0
            .get()
            .map(|pipelines| pipelines.fingerprint(id).is_some())
            .unwrap_or(false)
    }

    fn fingerprint(&self, id: &str) -> Option<String> {
        self.0.get().ok()?.fingerprint(id)
    }

    fn overwrite<'a>(
        &'a self,
        actor: &'a Actor,
        id: &'a str,
        yaml: &'a str,
        prompts: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<(), OverwriteRefusal>> {
        Box::pin(async move {
            let pipelines = self.0.get().map_err(OverwriteRefusal::Failed)?;
            pipelines
                .overwrite(actor, id, yaml, prompts)
                .await
                .map_err(|error| match error {
                    PipelineError::Denied(denied) => OverwriteRefusal::Denied(denied),
                    PipelineError::NotFound => OverwriteRefusal::NotFound,
                    PipelineError::Invalid(e) => {
                        OverwriteRefusal::Failed(format!("invalid run pipeline: {e}"))
                    }
                    PipelineError::Storage(e) => OverwriteRefusal::Failed(e),
                    other => OverwriteRefusal::Failed(format!("{other:?}")),
                })
        })
    }

    fn last_version<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Option<PipelineVersionRef>> {
        Box::pin(async move { self.0.get().ok()?.last_version(id).await })
    }
}
