//! A Pipeline's validation (CONTEXT.md « Pipeline validé »): the Projets whose
//! launch forms offer it. Data of the instance, never written into the YAML,
//! so an export carries no Projet.
//!
//! A Pipeline with no stated validation is validated for every Projet: that is
//! what keeps the Pipelines that existed before validations offered everywhere.
//! What PDO creates, duplicates or imports is born a *Pipeline de test*.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// On the wire: `{"kind":"all"}`, `{"kind":"none"}`,
/// `{"kind":"projects","project_ids":[…]}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum PipelineValidation {
    /// Every Projet, current and future. Also what an absent validation reads as.
    #[default]
    All,
    /// A *Pipeline de test*: offered only behind « Show test pipelines ».
    None,
    /// These Projets only, in the order they were given. Never empty once
    /// [`normalized`](PipelineValidation::normalized).
    Projects { project_ids: Vec<String> },
}

impl PipelineValidation {
    /// What is stored for what was asked: duplicates collapse (first one
    /// wins), and an empty Projet list is a test Pipeline (« nothing ticked »).
    pub fn normalized(self) -> Self {
        match self {
            Self::Projects { project_ids } => {
                let mut ids: Vec<String> = Vec::with_capacity(project_ids.len());
                for id in project_ids {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                if ids.is_empty() {
                    Self::None
                } else {
                    Self::Projects { project_ids: ids }
                }
            }
            other => other,
        }
    }

    /// The Projets it names; empty for `all` and `none`.
    pub fn project_ids(&self) -> &[String] {
        match self {
            Self::Projects { project_ids } => project_ids,
            _ => &[],
        }
    }

    /// The validation once the Projet `project_id` is deleted: the Projet is
    /// withdrawn, and a Pipeline left with no Projet becomes a test Pipeline
    /// rather than silently « all ». `None` when the Projet was not named.
    pub fn without_project(&self, project_id: &str) -> Option<Self> {
        let Self::Projects { project_ids } = self else {
            return None;
        };
        if !project_ids.iter().any(|id| id == project_id) {
            return None;
        }
        let kept = project_ids
            .iter()
            .filter(|id| *id != project_id)
            .cloned()
            .collect();
        Some(Self::Projects { project_ids: kept }.normalized())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(ids: &[&str]) -> PipelineValidation {
        PipelineValidation::Projects {
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    #[test]
    fn speaks_the_tagged_wire_shape() {
        let wire = |v: &PipelineValidation| serde_json::to_value(v).unwrap();
        assert_eq!(
            wire(&PipelineValidation::All),
            serde_json::json!({ "kind": "all" })
        );
        assert_eq!(
            wire(&PipelineValidation::None),
            serde_json::json!({ "kind": "none" })
        );
        assert_eq!(
            wire(&projects(&["p1"])),
            serde_json::json!({ "kind": "projects", "project_ids": ["p1"] })
        );
        let read: PipelineValidation =
            serde_json::from_value(serde_json::json!({ "kind": "projects", "project_ids": [] }))
                .unwrap();
        assert_eq!(read, projects(&[]));
    }

    #[test]
    fn an_absent_validation_is_every_projet() {
        assert_eq!(PipelineValidation::default(), PipelineValidation::All);
    }

    #[test]
    fn nothing_ticked_is_a_test_pipeline_and_duplicates_collapse() {
        assert_eq!(projects(&[]).normalized(), PipelineValidation::None);
        assert_eq!(
            projects(&["b", "a", "b"]).normalized(),
            projects(&["b", "a"])
        );
        assert_eq!(
            PipelineValidation::All.normalized(),
            PipelineValidation::All
        );
    }

    #[test]
    fn a_deleted_projet_is_withdrawn_and_the_last_one_leaves_a_test_pipeline() {
        assert_eq!(
            projects(&["a", "b"]).without_project("a"),
            Some(projects(&["b"]))
        );
        assert_eq!(
            projects(&["a"]).without_project("a"),
            Some(PipelineValidation::None)
        );
        assert_eq!(projects(&["a"]).without_project("z"), None);
        assert_eq!(PipelineValidation::All.without_project("a"), None);
        assert_eq!(PipelineValidation::None.without_project("a"), None);
    }
}
