//! What a Projet carries for the contexts downstream of the workspace (the
//! agentic profile, the skills selection), named with their real types. The
//! workspace stores both as opaque JSON; its HTTP adapter is generic over this.

use pdo_harness::agent_choice::AgentChoice;
use pdo_skill::domain::selection::{self as skill_selection, SkillRef};
use pdo_workspace::adapters::http::ProjectSettings;
use pdo_workspace::domain::project::Project;

/// The Projet tier of the agentic profile and of the skills selection.
pub struct ProjectTiers;

impl ProjectSettings for ProjectTiers {
    type AgentChoice = AgentChoice;
    type Skill = SkillRef;

    fn encode_skills(skills: Vec<Self::Skill>) -> Option<String> {
        skill_selection::to_stored_json(&skill_selection::normalise(skills))
    }

    fn decode_skills(stored: Option<&str>) -> Vec<Self::Skill> {
        skill_selection::from_stored_json(stored.map(str::to_string))
    }
}

/// The skills a Projet selects.
pub fn project_skills(project: &Project) -> Vec<SkillRef> {
    ProjectTiers::decode_skills(project.skills.as_ref().map(|s| s.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projet_settings_store_what_the_run_context_decodes() {
        let choice = AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        assert_eq!(
            ProjectTiers::encode_agent_choice(&choice),
            serde_json::to_string(&choice).unwrap()
        );
        let skills = ProjectTiers::encode_skills(vec![
            SkillRef {
                id: " tdd ".into(),
                name: "TDD".into(),
            },
            SkillRef {
                id: "tdd".into(),
                name: "dup".into(),
            },
        ]);
        let decoded = skill_selection::from_stored_json(skills);
        assert_eq!(decoded.len(), 1, "normalised on the way in: {decoded:?}");
        assert_eq!(decoded[0].id, "tdd");
        // An empty selection is stored as none.
        assert_eq!(ProjectTiers::encode_skills(vec![]), None);
    }
}
