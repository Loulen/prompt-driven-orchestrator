//! The entity attributes of the pipelines of the library (CONTEXT.md §
//! *Extensions*): stamped by the extensions on each pipeline an actor creates
//! (a new one, a duplicate, an import), carried over when a rename moves the
//! pipeline to a new id, and read with the list, which filters on them.

use pdo_kernel::attributes::{self, AttributeFilter, AttributedList, Attributes, EntityRef};
use pdo_kernel::{Actor, ResourceKind};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::queries::PipelineSummary;

impl Pipelines {
    /// Stamp the extensions' attributes on the pipeline `id`, which `actor`
    /// just created. Returns them.
    pub async fn stamp_created(&self, actor: &Actor, id: &str) -> Attributes {
        let entity = EntityRef::new(ResourceKind::Pipeline, id);
        attributes::stamp(self.host(), actor, entity).await
    }

    /// A rename moved the pipeline `from` to the id `to`: its attributes follow.
    pub async fn carry_attributes(&self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let carried = attributes::of_entity(self.host(), ResourceKind::Pipeline, from).await;
        if carried.is_empty() {
            return;
        }
        let store = self.host().attributes();
        let moved = store
            .put(&EntityRef::new(ResourceKind::Pipeline, to), &carried)
            .await;
        let dropped = store
            .forget(&EntityRef::new(ResourceKind::Pipeline, from))
            .await;
        if let Err(e) = moved.and(dropped) {
            tracing::error!("failed to carry the attributes of pipeline {from} to {to}: {e}");
        }
    }

    /// Every pipeline of the library carrying the attributes `filter` asks for,
    /// each with its attributes.
    pub async fn list_carrying(
        &self,
        filter: AttributeFilter,
    ) -> Result<Vec<(PipelineSummary, Attributes)>, PipelineError> {
        let pipelines = self.list()?;
        let attributed = AttributedList::read(self.host(), ResourceKind::Pipeline, filter).await;
        Ok(pipelines
            .into_iter()
            .filter(|summary| attributed.keeps(&summary.id))
            .map(|summary| {
                let carried = attributed.of(&summary.id);
                (summary, carried)
            })
            .collect())
    }
}
