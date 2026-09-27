//! Create a folder of the bank, at the root or under a parent.

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::folders::folder_name;
use crate::domain::skill::{new_folder_id, SkillFolder};

impl Skills {
    /// Folder names are free (only blank is refused): two sibling folders may
    /// share a name, the id tells them apart and nothing references a folder.
    pub async fn create_folder(
        &self,
        actor: &Actor,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<SkillFolder, SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::SkillFolder, Verb::Create),
        )?;
        self.new_folder(name, parent_id).await
    }

    pub(crate) async fn new_folder(
        &self,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<SkillFolder, SkillError> {
        let name = folder_name(name)?;
        let parent_id = self.existing_folder(parent_id).await?;
        let now = now_iso();
        let folder = SkillFolder {
            id: new_folder_id(),
            name,
            parent_id,
            source: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.folders.insert(&folder).await?;
        Ok(folder)
    }
}
