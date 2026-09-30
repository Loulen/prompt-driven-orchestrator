//! The seed of the extensions' skills at startup (#33, ADR-0081): each skill
//! an extension provides lands like PDO's own (`seed.rs`), in the folder of its
//! extension, locked and marked as provided by it. It is rewritten when the
//! extension's version or its content changes, and removed once the extension
//! no longer provides it. A transition of the daemon itself: no Autorisation
//! d'opération.

use pdo_kernel::clock::now_iso;

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::extension_skill::{extension_folder_id, ExtensionSkillSeed};
use crate::domain::file_path::reference_file;
use crate::domain::seed::SeedOutcome;
use crate::domain::skill::Skill;
use crate::domain::skill_md::validate_skill_md;

impl Skills {
    /// Seed every skill `extension` provides, then remove the ones it seeded
    /// before and no longer provides. One outcome per skill id, in order; a
    /// skipped skill never blocks the others.
    pub async fn seed_extension(
        &self,
        extension: &str,
        skills: &[ExtensionSkillSeed],
    ) -> Result<Vec<(String, SeedOutcome)>, SkillError> {
        let mut outcomes = Vec::with_capacity(skills.len());
        for skill in skills {
            outcomes.push((skill.id(), self.seed_extension_skill(skill).await?));
        }
        for stale in self.skills.list().await? {
            let provided = stale.extension.as_ref().is_some_and(|e| e.id == extension);
            if provided && !outcomes.iter().any(|(id, _)| *id == stale.id) {
                self.skills.remove(&stale.id).await?;
                self.content.remove(&stale.id)?;
                outcomes.push((stale.id, SeedOutcome::Removed));
            }
        }
        Ok(outcomes)
    }

    async fn seed_extension_skill(
        &self,
        seed: &ExtensionSkillSeed,
    ) -> Result<SeedOutcome, SkillError> {
        let skipped = |reason: String| Ok(SeedOutcome::Skipped { reason });
        // The extension's content passes the gate a pasted skill passes.
        let parsed = match validate_skill_md(&seed.skill_md) {
            Ok(parsed) => parsed,
            Err(error) => return skipped(format!("its SKILL.md is refused: {error}")),
        };
        if parsed.name != seed.name {
            return skipped(format!(
                "its SKILL.md is named `{}`, not `{}`",
                parsed.name, seed.name
            ));
        }
        let mut files = Vec::with_capacity(seed.files.len());
        for (path, content) in &seed.files {
            match reference_file(path, false) {
                Ok(rel) => files.push((rel, content.as_slice())),
                Err(error) => return skipped(format!("its file `{path}` is refused: {error}")),
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let id = seed.id();
        let now = now_iso();
        let folder_id = extension_folder_id(&seed.extension.id);
        let folder_pk = self
            .seed_folder(&folder_id, &seed.extension_name, &now)
            .await?;
        match self.skills.get(&id).await? {
            Some(mut skill) => {
                if skill.extension.as_ref() == Some(&seed.extension)
                    && self.holds(&id, &seed.skill_md, &files)
                {
                    return Ok(SeedOutcome::Unchanged);
                }
                self.content.remove(&id)?;
                self.write_seed(&id, &seed.skill_md, &files)?;
                skill.description = parsed.description;
                skill.folder_id = Some(folder_pk);
                skill.locked = true;
                skill.extension = Some(seed.extension.clone());
                skill.updated_at = now;
                self.skills.save(&skill).await?;
                Ok(SeedOutcome::Updated)
            }
            None => {
                if let Some(other) = self.skills.find_by_name(&parsed.name).await? {
                    return skipped(format!(
                        "a skill named `{}` already exists (id `{}`)",
                        other.name, other.id
                    ));
                }
                let skill = Skill {
                    id: id.clone(),
                    name: parsed.name,
                    description: parsed.description,
                    folder_id: Some(folder_pk),
                    locked: true,
                    extension: Some(seed.extension.clone()),
                    source: None,
                    created_at: now.clone(),
                    updated_at: now,
                };
                self.skills.insert(&skill).await?;
                if let Err(error) = self.write_seed(&id, &seed.skill_md, &files) {
                    let _ = self.skills.remove(&id).await;
                    let _ = self.content.remove(&id);
                    return Err(error);
                }
                Ok(SeedOutcome::Created)
            }
        }
    }

    /// Whether the folder of `id` holds exactly `skill_md` and `files`.
    fn holds(&self, id: &str, skill_md: &str, files: &[(String, &[u8])]) -> bool {
        if self.content.read_skill_md(id).ok().as_deref() != Some(skill_md) {
            return false;
        }
        let on_disk = match self.content.list_files(id) {
            Ok(on_disk) => on_disk,
            Err(_) => return false,
        };
        on_disk.len() == files.len()
            && on_disk.iter().zip(files).all(|(file, (rel, content))| {
                file.path == *rel
                    && self.content.read_file(id, rel).ok().flatten().as_deref() == Some(*content)
            })
    }

    fn write_seed(
        &self,
        id: &str,
        skill_md: &str,
        files: &[(String, &[u8])],
    ) -> Result<(), SkillError> {
        let failed = |error: SkillError| {
            SkillError::Storage(format!(
                "failed to write {}: {error}",
                self.content.dir(id).display()
            ))
        };
        self.content.write_skill_md(id, skill_md).map_err(failed)?;
        for (rel, content) in files {
            self.content.write_file(id, rel, content).map_err(failed)?;
        }
        Ok(())
    }
}
