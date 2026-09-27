//! Create a skill: from pasted `SKILL.md` text, from a folder found at a
//! Source, or under the id a pipeline document carries.
//!
//! Order matters and is the AC: validate the content, check the label, insert
//! the row, **then** write the folder — so a refusal never leaves a byte on
//! disk, and a disk failure rolls the row back.

use std::path::Path;

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::folders::folder_ref;
use crate::domain::skill::{is_safe_skill_id, new_skill_id, Provenance, Skill};
use crate::domain::skill_md::{normalise_label, validate_skill_md};

impl Skills {
    /// Create a skill from pasted `SKILL.md` text. `label` overrides the bank
    /// label (defaults to the frontmatter `name`).
    pub async fn create_skill(
        &self,
        actor: &Actor,
        content: &str,
        label: Option<&str>,
        folder_id: Option<&str>,
    ) -> Result<Skill, SkillError> {
        self.authorize(actor, Operation::new(ResourceKind::Skill, Verb::Create))?;
        self.index_new(None, content, label, folder_id).await
    }

    /// A label no other skill carries, case-insensitively (`excluding_id` may
    /// keep its own, whatever its case).
    pub(crate) async fn unique_label(
        &self,
        name: &str,
        excluding_id: Option<&str>,
    ) -> Result<String, SkillError> {
        let name = normalise_label(name)?;
        if let Some(existing) = self.skills.find_by_name(&name).await? {
            if Some(existing.id.as_str()) != excluding_id {
                return Err(SkillError::DuplicateName {
                    existing_id: existing.id,
                    name: existing.name,
                });
            }
        }
        Ok(name)
    }

    /// The folder a body names, which must exist; `None` for the root.
    pub(crate) async fn existing_folder(
        &self,
        folder_id: Option<&str>,
    ) -> Result<Option<String>, SkillError> {
        match folder_ref(folder_id) {
            Some(folder) => {
                if self.folders.get(folder).await?.is_none() {
                    return Err(SkillError::FolderNotFound);
                }
                Ok(Some(folder.to_string()))
            }
            None => Ok(None),
        }
    }

    /// Create from text, with the id chosen by the caller — the seam the
    /// document import uses (#673, ADR-0062 "identité par id"): a skill
    /// recreated from a sidecar keeps the id the pipeline references. `None`
    /// generates a fresh one. An id already indexed is a
    /// [`SkillError::Storage`] (the importer checks first and never
    /// overwrites); an unsafe id is an [`SkillError::InvalidPath`].
    pub(crate) async fn index_new(
        &self,
        id: Option<&str>,
        content: &str,
        label: Option<&str>,
        folder_id: Option<&str>,
    ) -> Result<Skill, SkillError> {
        if let Some(id) = id {
            if !is_safe_skill_id(id) {
                return Err(SkillError::InvalidPath(id.to_string()));
            }
            if self.skills.get(id).await?.is_some() {
                return Err(SkillError::Storage(format!(
                    "a skill with id `{id}` already exists"
                )));
            }
        }
        let parsed = validate_skill_md(content)?;
        let name = self
            .unique_label(label.unwrap_or(&parsed.name), None)
            .await?;
        let folder_id = self.existing_folder(folder_id).await?;
        let now = now_iso();
        let skill = Skill {
            id: id.map(str::to_string).unwrap_or_else(new_skill_id),
            name,
            description: parsed.description,
            folder_id,
            locked: false,
            source: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.skills.insert(&skill).await?;
        if let Err(error) = self.content.write_skill_md(&skill.id, content) {
            let _ = self.content.remove(&skill.id);
            let _ = self.skills.remove(&skill.id).await;
            return Err(SkillError::Storage(format!(
                "failed to write {}: {error}",
                self.content.dir(&skill.id).display()
            )));
        }
        Ok(skill)
    }

    /// Create from a **folder** (the import path, #670): same gate and same
    /// order — validate the `SKILL.md`, check the label, insert the row with its
    /// provenance, then copy the whole folder. A refusal leaves no byte on disk;
    /// a copy failure rolls the row back.
    pub(crate) async fn index_from_dir(
        &self,
        src_dir: &Path,
        label: Option<&str>,
        folder_id: Option<&str>,
        provenance: &Provenance,
    ) -> Result<Skill, SkillError> {
        let content = self.scanner.read_skill_md(src_dir)?;
        let parsed = validate_skill_md(&content)?;
        let name = self
            .unique_label(label.unwrap_or(&parsed.name), None)
            .await?;
        let folder_id = self.existing_folder(folder_id).await?;
        let now = now_iso();
        let skill = Skill {
            id: new_skill_id(),
            name,
            description: parsed.description,
            folder_id,
            locked: false,
            source: Some(provenance.clone()),
            created_at: now.clone(),
            updated_at: now,
        };
        self.skills.insert(&skill).await?;
        if let Err(error) = self.content.copy_from(&skill.id, src_dir) {
            let _ = self.skills.remove(&skill.id).await;
            return Err(error);
        }
        Ok(skill)
    }

    /// Replace a skill's **content** with a folder from a source, keeping its
    /// id, label, folder and referents (the *Replace* choice of a collision, and
    /// the *updated* rows of an Update from source). The old folder is swapped
    /// out only after the new content validated.
    pub(crate) async fn replace_from_dir(
        &self,
        id: &str,
        src_dir: &Path,
        provenance: &Provenance,
    ) -> Result<Skill, SkillError> {
        let mut skill = self.skills.get(id).await?.ok_or(SkillError::NotFound)?;
        let content = self.scanner.read_skill_md(src_dir)?;
        let parsed = validate_skill_md(&content)?;
        self.content.replace_from(id, src_dir)?;
        skill.description = parsed.description;
        skill.source = Some(provenance.clone());
        skill.updated_at = now_iso();
        self.skills.save(&skill).await?;
        self.skills.get(id).await?.ok_or(SkillError::NotFound)
    }
}
