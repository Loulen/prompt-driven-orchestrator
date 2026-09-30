//! The seeded skills (#722, spec #719, ADR-0064; #588, ADR-0069). One per node
//! toggle: `orchestrator` seeds `pdo-orchestrate`, `interactive` seeds
//! `pdo-interactive`. They land at every daemon start, into one « PDO »
//! folder, and are locked against every bank write.

use super::error::SkillError;

pub const SEEDED_SKILL_ID: &str = "pdo-orchestrate";
pub const INTERACTIVE_SKILL_ID: &str = "pdo-interactive";
pub const SEEDED_FOLDER_ID: &str = "skf-pdo";
pub const SEEDED_FOLDER_NAME: &str = "PDO";

/// The seeded `pdo-orchestrate` `SKILL.md`, versioned with this binary. Bump
/// `skill_version` in the frontmatter whenever the guidance changes: the next
/// start overwrites the copy in every bank, keeping ids and referents intact.
///
/// v3 (#588 / ADR-0069): « Waiting for your children » around `pdo run wait`
/// (no more periodic polling), and the binding contract moved here from the
/// preamble's Orchestration amendment (now the bare `/pdo-orchestrate` line).
pub const SEEDED_SKILL_MD: &str = include_str!("seed/pdo-orchestrate.md");

/// The seeded `pdo-interactive` `SKILL.md` (#588 / ADR-0069): how a node whose
/// `interactive` toggle is on conducts itself — declare the wait, never block
/// the conversation, the release / exit-3 flow.
pub const INTERACTIVE_SKILL_MD: &str = include_str!("seed/pdo-interactive.md");

/// One skill PDO seeds at startup: its bank id (also its name) and the built-in
/// `SKILL.md`. The seed carries a **list** (ADR-0069 §2): adding a toggle skill
/// is one more entry here, nothing else.
pub struct SeededSkill {
    pub id: &'static str,
    pub content: &'static str,
}

pub const SEEDED_SKILLS: &[SeededSkill] = &[
    SeededSkill {
        id: SEEDED_SKILL_ID,
        content: SEEDED_SKILL_MD,
    },
    SeededSkill {
        id: INTERACTIVE_SKILL_ID,
        content: INTERACTIVE_SKILL_MD,
    },
];

/// Is `id` one of the seeded skills? The one question the lock and the UI ask.
pub fn is_seeded(id: &str) -> bool {
    SEEDED_SKILLS.iter().any(|skill| skill.id == id)
}

/// The seed lock (#722): every write verb on a seeded skill is refused with
/// the same named error, wherever it comes from — the panel, the endpoints, an
/// import. The seed itself never passes here.
pub fn refuse_locked(id: &str) -> Result<(), SkillError> {
    if is_seeded(id) {
        return Err(SkillError::Locked { id: id.to_string() });
    }
    Ok(())
}

/// What one pass of the seed did — logged at startup, asserted in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedOutcome {
    /// The skill was absent: row and folder written.
    Created,
    /// The content on disk differed from the built-in copy: overwritten.
    Updated,
    /// Row and content already matched: nothing written.
    Unchanged,
    /// The seed could not claim its identity (another skill owns the label),
    /// or an extension's skill does not pass the gate: nothing written, the
    /// daemon runs without the seed.
    Skipped { reason: String },
    /// An extension's skill it no longer provides: row and content removed.
    Removed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skill_md::validate_skill_md;

    #[test]
    fn both_seeded_skills_pass_the_gate_and_carry_their_guidance() {
        for skill in SEEDED_SKILLS {
            assert_eq!(validate_skill_md(skill.content).unwrap().name, skill.id);
        }
        assert!(INTERACTIVE_SKILL_MD.contains("pdo wait-user --message"));
        assert!(INTERACTIVE_SKILL_MD.contains("Mark ready for completion"));
        // The orchestrate skill's v3 guidance is the blocking pull, no polling.
        assert!(SEEDED_SKILL_MD.contains("pdo run wait"));
        assert!(!SEEDED_SKILL_MD.contains("Check it periodically"));
        assert!(SEEDED_SKILL_MD.contains("skill_version: 3"));
    }

    #[test]
    fn only_the_seeded_skills_are_locked() {
        assert!(is_seeded(SEEDED_SKILL_ID) && is_seeded(INTERACTIVE_SKILL_ID));
        assert!(!is_seeded("tdd"));
        assert!(matches!(
            refuse_locked(INTERACTIVE_SKILL_ID),
            Err(SkillError::Locked { .. })
        ));
        assert_eq!(refuse_locked("tdd"), Ok(()));
    }
}
