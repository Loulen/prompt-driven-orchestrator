//! Remove one reference file of a skill (never its `SKILL.md`), then prune the
//! sub-folders it leaves empty — the skill folder itself stays.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::file_path::reference_file;

impl Skills {
    pub async fn delete_file(&self, actor: &Actor, id: &str, path: &str) -> Result<(), SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Update).on(id),
        )?;
        if self.skills.get(id).await?.is_none() {
            return Err(SkillError::NotFound);
        }
        self.refuse_locked(id).await?;
        let rel = reference_file(path, false)?;
        if !self.content.is_file(id, &rel) {
            return Err(SkillError::FileNotFound(rel));
        }
        self.content.delete_file(id, &rel)
    }
}
