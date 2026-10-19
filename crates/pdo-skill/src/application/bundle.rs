//! Export skills of the bank as a zip, and upload such a zip back into a bank
//! (see [`crate::domain::bundle`]).

use pdo_kernel::Actor;

use super::skills::Skills;
use crate::domain::bundle::{bundle_name_of_file, bundle_selection};
use crate::domain::error::SkillError;
use crate::domain::selection::SkillRef;
use crate::domain::sidecar::{SidecarImportReport, SidecarSkill};
use crate::domain::skill_md::validate_skill_md;

/// What an export ships: the zip's name and every skill's files.
#[derive(Debug, Clone)]
pub struct BankExport {
    pub name: String,
    pub skills: Vec<SidecarSkill>,
}

impl Skills {
    /// The skills `skill_ids` names and every skill under `folder_ids`, read
    /// from the bank. A read: no Operation is asked.
    pub async fn export_skills(
        &self,
        skill_ids: &[String],
        folder_ids: &[String],
    ) -> Result<BankExport, SkillError> {
        let skills = self.skills.list().await?;
        let folders = self.folders.list().await?;
        let selection = bundle_selection(&skills, &folders, skill_ids, folder_ids)?;
        let (carried, _absent) = self.sidecar_of(&selection.skills);
        if carried.is_empty() {
            return Err(SkillError::NothingToExport);
        }
        Ok(BankExport {
            name: selection.name,
            skills: carried,
        })
    }

    /// Upload a zip the export wrote (or any sidecar), `archive` as the zip
    /// reader parsed it: every skill it carries goes through the sidecar's
    /// import policy, the created ones filed under a root folder named after
    /// `file_name`. The Operation is asked first: a refused actor learns
    /// nothing of the archive.
    pub async fn upload_bundle(
        &self,
        actor: &Actor,
        file_name: &str,
        archive: Result<Vec<SidecarSkill>, String>,
    ) -> Result<SidecarImportReport, SkillError> {
        self.authorize_import(actor)?;
        let carried = archive.map_err(SkillError::InvalidArchive)?;
        if carried.is_empty() {
            return Err(SkillError::InvalidArchive(
                "the zip holds no skill: expected one folder per skill with its SKILL.md".into(),
            ));
        }
        let references = carried
            .iter()
            .map(|skill| SkillRef {
                id: skill.id.clone(),
                name: skill
                    .skill_md()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|text| validate_skill_md(text).ok())
                    .map(|parsed| parsed.name)
                    .unwrap_or_default(),
            })
            .collect();
        self.import_carried(&bundle_name_of_file(file_name), references, &carried)
            .await
    }
}
