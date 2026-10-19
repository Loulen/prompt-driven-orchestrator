//! A Pipeline's validation (CONTEXT.md « Pipeline validé »): the Projets whose
//! launch forms offer it. Data of the instance, never written into the YAML,
//! so an export carries no Projet.
//!
//! A Pipeline with no stated validation is validated for every Projet: that is
//! what keeps the Pipelines that existed before validations offered everywhere.
//! What PDO creates, duplicates or imports is born a *Pipeline de test*.
//!
//! A Pipeline validated for every Projet may also be *mis en tête* for some of
//! them: those Projets list it among their own Pipelines, before the global ones.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// On the wire: `{"kind":"all"}` (or `{"kind":"all","project_ids":[…]}` when
/// it is listed first for some Projets), `{"kind":"none"}`,
/// `{"kind":"projects","project_ids":[…]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum PipelineValidation {
    /// Every Projet, current and future. Also what an absent validation reads as.
    /// `project_ids` are the Projets it is listed first for, in the order
    /// given; absent reads as none, and none is not written.
    All {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(as = "Option<Vec<String>>", optional)]
        project_ids: Vec<String>,
    },
    /// A *Pipeline de test*: offered only behind « Show test pipelines ».
    None,
    /// These Projets only, in the order they were given. Never empty once
    /// [`normalized`](PipelineValidation::normalized).
    Projects { project_ids: Vec<String> },
}

impl Default for PipelineValidation {
    fn default() -> Self {
        Self::all()
    }
}

impl PipelineValidation {
    /// Every Projet, listed first for none.
    pub fn all() -> Self {
        Self::All {
            project_ids: Vec::new(),
        }
    }

    /// What is stored for what was asked: duplicates collapse (first one
    /// wins), and an empty Projet list is a test Pipeline (« nothing ticked »)
    /// — unless it is validated for every Projet, which stays so.
    pub fn normalized(self) -> Self {
        match self {
            Self::Projects { project_ids } => {
                let ids = dedup(project_ids);
                if ids.is_empty() {
                    Self::None
                } else {
                    Self::Projects { project_ids: ids }
                }
            }
            Self::All { project_ids } => Self::All {
                project_ids: dedup(project_ids),
            },
            Self::None => Self::None,
        }
    }

    /// The Projets it names: those it is offered to, or listed first for when
    /// it is validated for every Projet; empty for `none`.
    pub fn project_ids(&self) -> &[String] {
        match self {
            Self::Projects { project_ids } | Self::All { project_ids } => project_ids,
            Self::None => &[],
        }
    }

    /// The validation once the Projet `project_id` is deleted: the Projet is
    /// withdrawn. A Pipeline validated for every Projet stays so; one left
    /// with no Projet becomes a test Pipeline rather than silently « all ».
    /// `None` when the Projet was not named.
    pub fn without_project(&self, project_id: &str) -> Option<Self> {
        let (Self::Projects { project_ids } | Self::All { project_ids }) = self else {
            return None;
        };
        if !project_ids.iter().any(|id| id == project_id) {
            return None;
        }
        let kept: Vec<String> = project_ids
            .iter()
            .filter(|id| *id != project_id)
            .cloned()
            .collect();
        Some(match self {
            Self::All { .. } => Self::All { project_ids: kept },
            _ => Self::Projects { project_ids: kept }.normalized(),
        })
    }
}

/// Duplicates collapse, the first one wins.
fn dedup(project_ids: Vec<String>) -> Vec<String> {
    let mut ids: Vec<String> = Vec::with_capacity(project_ids.len());
    for id in project_ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(ids: &[&str]) -> PipelineValidation {
        PipelineValidation::Projects {
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn all(ids: &[&str]) -> PipelineValidation {
        PipelineValidation::All {
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    #[test]
    fn speaks_the_tagged_wire_shape() {
        let wire = |v: &PipelineValidation| serde_json::to_value(v).unwrap();
        assert_eq!(
            wire(&PipelineValidation::all()),
            serde_json::json!({ "kind": "all" })
        );
        assert_eq!(
            wire(&all(&["p1"])),
            serde_json::json!({ "kind": "all", "project_ids": ["p1"] })
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
    fn every_projet_reads_its_listed_first_projets_and_an_absent_list_as_none() {
        let read = |v: serde_json::Value| serde_json::from_value::<PipelineValidation>(v).unwrap();
        assert_eq!(read(serde_json::json!({ "kind": "all" })), all(&[]));
        assert_eq!(
            read(serde_json::json!({ "kind": "all", "project_ids": ["a", "b"] })),
            all(&["a", "b"])
        );
    }

    #[test]
    fn an_absent_validation_is_every_projet() {
        assert_eq!(PipelineValidation::default(), all(&[]));
    }

    #[test]
    fn nothing_ticked_is_a_test_pipeline_and_duplicates_collapse() {
        assert_eq!(projects(&[]).normalized(), PipelineValidation::None);
        assert_eq!(
            projects(&["b", "a", "b"]).normalized(),
            projects(&["b", "a"])
        );
        assert_eq!(all(&[]).normalized(), all(&[]));
        assert_eq!(all(&["b", "a", "b"]).normalized(), all(&["b", "a"]));
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
        assert_eq!(all(&[]).without_project("a"), None);
        assert_eq!(PipelineValidation::None.without_project("a"), None);
    }

    #[test]
    fn a_deleted_projet_leaves_every_projet_as_every_projet() {
        assert_eq!(all(&["a", "b"]).without_project("a"), Some(all(&["b"])));
        assert_eq!(all(&["a"]).without_project("a"), Some(all(&[])));
        assert_eq!(all(&["a"]).without_project("z"), None);
    }
}
