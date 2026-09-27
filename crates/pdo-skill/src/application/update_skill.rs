//! Rename (label only) and/or move a skill. Renaming never touches the disk:
//! the folder carries the id, the frontmatter `name` stays (#668 AC).

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::folders::folder_ref;
use crate::domain::seed::refuse_locked;
use crate::domain::skill::Skill;

impl Skills {
    /// A sparse edit of the index row: `name` renames, `folder_id` moves
    /// (`Some(None)` = back to the root).
    pub async fn update_skill(
        &self,
        actor: &Actor,
        id: &str,
        name: Option<&str>,
        folder_id: Option<Option<&str>>,
    ) -> Result<Skill, SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Update).on(id),
        )?;
        refuse_locked(id)?;
        let mut skill = self.skills.get(id).await?.ok_or(SkillError::NotFound)?;
        if let Some(candidate) = name {
            skill.name = self.unique_label(candidate, Some(id)).await?;
        }
        match folder_id {
            None => {}
            Some(folder) => match folder_ref(folder) {
                None => skill.folder_id = None,
                Some(folder) => skill.folder_id = self.existing_folder(Some(folder)).await?,
            },
        }
        skill.updated_at = now_iso();
        self.skills.save(&skill).await?;
        self.skills.get(id).await?.ok_or(SkillError::NotFound)
    }
}
