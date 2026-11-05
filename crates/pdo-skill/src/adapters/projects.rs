//! The skill context's word on a Projet's skills selection, which the
//! workspace stores as opaque JSON and never reads.

use std::sync::Arc;

use pdo_workspace::application::ports::{BoxFuture, SkillSelections, StorageError, StoreResult};
use pdo_workspace::domain::project::StoredSetting;

use crate::application::Skills;
use crate::domain::selection::{foreign_skill_refusal, SkillRef};

/// A Projet's selection only gains skills of the bank (`Skills::foreign_skill`).
pub struct ProjectSkillSelections(pub Arc<Skills>);

impl SkillSelections for ProjectSkillSelections {
    fn refusal<'a>(
        &'a self,
        current: Option<&'a StoredSetting>,
        next: &'a StoredSetting,
    ) -> BoxFuture<'a, StoreResult<Option<String>>> {
        Box::pin(async move {
            let current = current.map(selection).unwrap_or_default();
            let foreign = self
                .0
                .foreign_skill(&current, &selection(next))
                .await
                .map_err(|e| StorageError(e.to_string()))?;
            Ok(foreign.as_deref().map(foreign_skill_refusal))
        })
    }
}

/// The skills a stored selection names; none when it is not one. The route
/// types a selection before the workspace stores it, so an unreadable one
/// never reaches here from the wire; read as none, it gains no skill.
fn selection(stored: &StoredSetting) -> Vec<SkillRef> {
    serde_json::from_str(stored.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_selection_reads_as_its_skills_and_anything_else_as_none() {
        let read = selection(&StoredSetting(
            r#"[{"id":"tdd","name":"TDD"},{"id":"plan"}]"#.into(),
        ));
        let ids: Vec<&str> = read.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["tdd", "plan"]);
        assert!(selection(&StoredSetting("not json".into())).is_empty());
    }
}
