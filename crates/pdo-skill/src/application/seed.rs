//! The seed at startup (#722, ADR-0064): every seeded skill lands, created if
//! absent, its content rewritten when the built-in copy differs. A transition
//! of the daemon itself, not an actor's gesture: no Autorisation d'opération.

use pdo_kernel::clock::now_iso;

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::seed::{SeedOutcome, SEEDED_FOLDER_ID, SEEDED_FOLDER_NAME, SEEDED_SKILLS};
use crate::domain::skill::{Skill, SkillFolder};
use crate::domain::skill_md::validate_skill_md;

impl Skills {
    /// One outcome per seeded skill, in order, into the one « PDO » folder. A
    /// skipped skill (its label owned by a user skill) never blocks the others.
    pub async fn seed(&self) -> Result<Vec<(&'static str, SeedOutcome)>, SkillError> {
        let mut outcomes = Vec::with_capacity(SEEDED_SKILLS.len());
        for skill in SEEDED_SKILLS {
            let outcome = self
                .seed_with(
                    skill.id,
                    SEEDED_FOLDER_ID,
                    SEEDED_FOLDER_NAME,
                    skill.content,
                )
                .await?;
            outcomes.push((skill.id, outcome));
        }
        Ok(outcomes)
    }

    /// The one seed pass, parameterised for tests. Idempotent: `Created` once,
    /// then `Unchanged` until the built-in content changes.
    pub async fn seed_with(
        &self,
        id: &str,
        folder_id: &str,
        folder_name: &str,
        content: &str,
    ) -> Result<SeedOutcome, SkillError> {
        // Our own content passes the same gate a pasted SKILL.md would: the seed
        // never writes a skill the harness would ignore.
        let parsed = validate_skill_md(content)?;
        let now = now_iso();
        let folder_pk = self.seed_folder(folder_id, folder_name, &now).await?;
        match self.skills.get(id).await? {
            Some(mut skill) => {
                // Re-seed whenever the disk content is not exactly the built-in
                // copy: a version bump, an erased file, a corrupted folder. The
                // id and the references that select the skill stay put.
                if self.content.read_skill_md(id).ok().as_deref() == Some(content) {
                    return Ok(SeedOutcome::Unchanged);
                }
                self.content.write_skill_md(id, content)?;
                skill.description = parsed.description;
                skill.folder_id = Some(folder_pk);
                skill.updated_at = now;
                self.skills.save(&skill).await?;
                Ok(SeedOutcome::Updated)
            }
            None => {
                // The label is unique in the bank: if a user skill already
                // carries it, skip rather than fight the index — the operator
                // renames theirs, and the next start seeds.
                if let Some(other) = self.skills.find_by_name(&parsed.name).await? {
                    return Ok(SeedOutcome::Skipped {
                        reason: format!(
                            "a skill named `{}` already exists (id `{}`)",
                            other.name, other.id
                        ),
                    });
                }
                // Validate, index, then write — a disk failure rolls the row
                // back, leaving no half seed.
                let skill = Skill {
                    id: id.to_string(),
                    name: parsed.name,
                    description: parsed.description,
                    folder_id: Some(folder_pk),
                    locked: true,
                    source: None,
                    created_at: now.clone(),
                    updated_at: now,
                };
                self.skills.insert(&skill).await?;
                if let Err(error) = self.content.write_skill_md(id, content) {
                    let _ = self.skills.remove(id).await;
                    return Err(SkillError::Storage(format!(
                        "failed to write {}: {error}",
                        self.content.dir(id).display()
                    )));
                }
                Ok(SeedOutcome::Created)
            }
        }
    }

    /// The « PDO » folder: a fixed id, or an existing root folder already
    /// carrying the name (an operator's folder named PDO is adopted, not
    /// duplicated — the folder is a UI gesture, nothing references it).
    async fn seed_folder(
        &self,
        folder_id: &str,
        folder_name: &str,
        now: &str,
    ) -> Result<String, SkillError> {
        if self.folders.get(folder_id).await?.is_some() {
            return Ok(folder_id.to_string());
        }
        if let Some(existing) = self.folders.root_named(folder_name).await? {
            return Ok(existing);
        }
        self.folders
            .insert(&SkillFolder {
                id: folder_id.to_string(),
                name: folder_name.to_string(),
                parent_id: None,
                source: None,
                created_at: now.to_string(),
                updated_at: now.to_string(),
            })
            .await?;
        Ok(folder_id.to_string())
    }
}
