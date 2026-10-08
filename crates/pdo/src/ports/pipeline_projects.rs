//! The pipelines' validations and the Projets (CONTEXT.md « Pipeline
//! validé »): the pipeline context asks whether a Projet exists before it
//! validates a pipeline for it, and the workspace context has the validations
//! forget a Projet it deleted. The pipeline context is wired after the
//! workspace, so the deletion reaches it through a [`Late`] handle.

use std::sync::Arc;

use pdo_pipeline::application::ports::{BoxFuture as PipelineFuture, KnownProjects};
use pdo_pipeline::Pipelines;
use pdo_workspace::application::ports::{
    BoxFuture as WorkspaceFuture, ProjectDeletions, ProjectRepository,
};
use tracing::warn;

use super::Late;

/// The Projets as the workspace context stores them.
pub(crate) struct WorkspaceKnownProjects(pub(crate) Arc<dyn ProjectRepository>);

impl KnownProjects for WorkspaceKnownProjects {
    fn exists<'a>(&'a self, project_id: &'a str) -> PipelineFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            self.0
                .get(project_id)
                .await
                .map(|project| project.is_some())
                .map_err(|e| e.to_string())
        })
    }
}

/// A deleted Projet, withdrawn from the validations of the pipelines.
pub(crate) struct ValidationsForgetProjects(pub(crate) Late<Pipelines>);

impl ProjectDeletions for ValidationsForgetProjects {
    fn project_deleted<'a>(&'a self, project_id: &'a str) -> WorkspaceFuture<'a, ()> {
        Box::pin(async move {
            match self.0.get() {
                Ok(pipelines) => pipelines.forget_project(project_id).await,
                Err(e) => warn!("Projet {project_id} not withdrawn from the validations: {e}"),
            }
        })
    }
}
