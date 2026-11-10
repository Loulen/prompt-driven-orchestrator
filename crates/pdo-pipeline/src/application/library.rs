//! Star a node into the library, and take it out.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::error::PipelineError;
use super::pipelines::Pipelines;
use crate::domain::library::LibraryEntry;

impl Pipelines {
    /// Save `entry` (an entry of the same name is overwritten) and answer it
    /// as the library recorded it: its isolation stated (#655), even when the
    /// client said nothing.
    pub fn save_to_library(
        &self,
        actor: &Actor,
        mut entry: LibraryEntry,
    ) -> Result<LibraryEntry, PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::LibraryEntry, Verb::Create).on(entry.name.as_str()),
        )?;
        entry.stamp_isolation();
        self.library.save(&entry).map_err(PipelineError::Storage)?;
        Ok(entry)
    }

    /// Take the entry `name` out of the library.
    pub fn delete_from_library(&self, actor: &Actor, name: &str) -> Result<(), PipelineError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::LibraryEntry, Verb::Delete).on(name),
        )?;
        match self.library.delete(name) {
            Ok(true) => Ok(()),
            Ok(false) => Err(PipelineError::NotFound),
            Err(e) => Err(PipelineError::Storage(e)),
        }
    }
}
