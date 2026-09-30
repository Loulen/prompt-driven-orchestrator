//! Delete a skill: row first, then its folder on disk. **Unconditional** — no
//! referential integrity, the referents dialog informs the confirmation (same
//! posture as the agentic profiles).

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;

impl Skills {
    /// `false` when the id is unknown.
    pub async fn delete_skill(&self, actor: &Actor, id: &str) -> Result<bool, SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Delete).on(id),
        )?;
        self.refuse_locked(id).await?;
        if !self.skills.remove(id).await? {
            return Ok(false);
        }
        self.content.remove(id)?;
        Ok(true)
    }
}
