//! Delete a folder of the bank. Its skills and sub-folders move to its parent:
//! deleting a folder **never deletes a skill**.

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;

impl Skills {
    /// `false` when the folder is unknown.
    pub async fn delete_folder(&self, actor: &Actor, id: &str) -> Result<bool, SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::SkillFolder, Verb::Delete).on(id),
        )?;
        self.remove_folder(id).await
    }

    pub(crate) async fn remove_folder(&self, id: &str) -> Result<bool, SkillError> {
        let Some(folder) = self.folders.get(id).await? else {
            return Ok(false);
        };
        let now = now_iso();
        let parent = folder.parent_id.as_deref();
        self.skills.refile(id, parent, &now).await?;
        self.folders.reparent(id, parent, &now).await?;
        self.folders.remove(id).await?;
        Ok(true)
    }
}
