//! Who names an agentic profile: the instance tier, the Projets, the Triggers
//! and the nodes of the instance's pipelines. What the delete dialog lists
//! before the operator confirms — a delete is unconditional (no referential
//! integrity in the database, ADR-0031 §7), the list is what informs it.

use pdo_harness::agent_choice::AgentChoice;
use serde::Serialize;

/// Whether `choice` selects the profile `profile_id` (a `Custom` choice names
/// no profile).
pub fn names_profile(choice: Option<&AgentChoice>, profile_id: &str) -> bool {
    matches!(choice, Some(AgentChoice::Profile { profile_id: id }) if id == profile_id)
}

/// A Projet naming the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectReferent {
    pub id: String,
    pub name: String,
}

/// A Trigger naming the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggerReferent {
    pub id: String,
    pub name: String,
    pub pipeline_id: String,
}

/// A node of an instance pipeline naming the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipelineReferent {
    pub id: String,
    pub name: String,
    pub node_id: String,
    pub scope: String,
}

/// Every referent of one profile. The Runs are never listed: a Run froze its
/// combination when it started, a delete cannot reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentProfileReferents {
    pub profile_id: String,
    pub instance: bool,
    pub projects: Vec<ProjectReferent>,
    pub triggers: Vec<TriggerReferent>,
    pub pipelines: Vec<PipelineReferent>,
    pub runs: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_profile_choice_with_that_id_names_it() {
        let profile = AgentChoice::Profile {
            profile_id: "agp-1".into(),
        };
        assert!(names_profile(Some(&profile), "agp-1"));
        assert!(!names_profile(Some(&profile), "agp-2"));
        assert!(!names_profile(None, "agp-1"));
    }
}
