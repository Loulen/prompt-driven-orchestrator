//! Save the plain-text editor (#671): the `SKILL.md` goes through the five
//! checks again, a reference file is overwritten as is.

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::file_path::{normalise_file_path, reference_file};
use crate::domain::seed::refuse_locked;
use crate::domain::skill::{Skill, SkillFile, SKILL_MD};
use crate::domain::skill_md::validate_skill_md;

/// What a save wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedFile {
    /// The `SKILL.md`: the row follows its new description.
    SkillMd {
        size: usize,
        skill: Box<Skill>,
    },
    Reference(SkillFile),
}

impl Skills {
    /// Save `text` as the file `path` of the skill `id`.
    pub async fn save_file(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
        text: &str,
    ) -> Result<SavedFile, SkillError> {
        let rel = normalise_file_path(path)?;
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Update).on(id),
        )?;
        if rel == SKILL_MD {
            let skill = self.replace_skill_md(id, text).await?;
            return Ok(SavedFile::SkillMd {
                size: text.len(),
                skill: Box::new(skill),
            });
        }
        if self.skills.get(id).await?.is_none() {
            return Err(SkillError::NotFound);
        }
        refuse_locked(id)?;
        let rel = reference_file(&rel, false)?;
        if !self.content.is_file(id, &rel) {
            return Err(SkillError::FileNotFound(rel));
        }
        self.write_reference_file(id, &rel, text.as_bytes())
            .map(SavedFile::Reference)
    }

    /// Replace the `SKILL.md` of an existing skill (editor save, or a dropped
    /// `SKILL.md` — #671 design: "replaces the text, no confirmation, the five
    /// checks re-run"). An invalid text is refused with its named reason and
    /// **nothing is written**. The row's `description` follows the new
    /// frontmatter; the label (`name`) does not — renaming is its own verb.
    async fn replace_skill_md(&self, id: &str, content: &str) -> Result<Skill, SkillError> {
        refuse_locked(id)?;
        let mut skill = self.skills.get(id).await?.ok_or(SkillError::NotFound)?;
        let parsed = validate_skill_md(content)?;
        self.content.write_skill_md(id, content)?;
        skill.description = parsed.description;
        skill.updated_at = now_iso();
        self.skills.save(&skill).await?;
        self.skills.get(id).await?.ok_or(SkillError::NotFound)
    }
}
