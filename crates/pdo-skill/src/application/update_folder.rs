//! Rename and/or re-parent a folder of the bank.

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::folders::{folder_name, is_self_or_descendant};
use crate::domain::skill::SkillFolder;

impl Skills {
    /// `parent_id: Some(None)` moves the folder to the root. A move under
    /// itself or one of its descendants is a [`SkillError::FolderCycle`].
    pub async fn update_folder(
        &self,
        actor: &Actor,
        id: &str,
        name: Option<&str>,
        parent_id: Option<Option<&str>>,
    ) -> Result<SkillFolder, SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::SkillFolder, Verb::Update).on(id),
        )?;
        let mut folder = self
            .folders
            .get(id)
            .await?
            .ok_or(SkillError::FolderNotFound)?;
        if let Some(candidate) = name {
            folder.name = folder_name(candidate)?;
        }
        if let Some(parent) = parent_id {
            let parent = self.existing_folder(parent).await?;
            if let Some(parent) = &parent {
                let folders = self.folders.list().await?;
                if is_self_or_descendant(&folders, id, parent) {
                    return Err(SkillError::FolderCycle);
                }
            }
            folder.parent_id = parent;
        }
        folder.updated_at = now_iso();
        self.folders.save(&folder).await?;
        self.folders
            .get(id)
            .await?
            .ok_or(SkillError::FolderNotFound)
    }
}
