//! The pipelines' validations and the Projets (CONTEXT.md « Pipeline
//! validé »): the pipeline context asks whether a Projet exists before it
//! validates a pipeline for it, and the workspace context has the validations
//! — and the Triggers launching in it (ADR-0087) — forget a Projet it
//! deleted. The pipeline context is wired after the workspace, so the
//! deletion reaches it through a [`Late`] handle.

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

/// A deleted Projet, withdrawn from the validations of the pipelines and
/// from the Triggers launching their Runs in it (they launch in none).
pub(crate) struct ForgetDeletedProjects {
    pub(crate) pipelines: Late<Pipelines>,
    pub(crate) db: sqlx::SqlitePool,
}

impl ProjectDeletions for ForgetDeletedProjects {
    fn project_deleted<'a>(&'a self, project_id: &'a str) -> WorkspaceFuture<'a, ()> {
        Box::pin(async move {
            match self.pipelines.get() {
                Ok(pipelines) => pipelines.forget_project(project_id).await,
                Err(e) => warn!("Projet {project_id} not withdrawn from the validations: {e}"),
            }
            if let Err(e) =
                pdo_trigger::adapters::sqlite::forget_project(&self.db, project_id).await
            {
                warn!("Projet {project_id} not withdrawn from the triggers: {e}");
            }
        })
    }
}
