//! Re-scan a Source folder's source and diff it against the folder's skills.
//! Read-only on the bank.

use std::collections::HashSet;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::import_error::ImportError;
use crate::domain::source::source_of_folder;
use crate::domain::update::{
    from_source, gone_entry, known_entry, matched_skill, new_entry, RescanReport, TreeDiff,
};

impl Skills {
    pub async fn rescan_folder(
        &self,
        actor: &Actor,
        folder_id: &str,
        scan_id: &str,
    ) -> Result<RescanReport, ImportError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::SkillFolder, Verb::Action("rescan".into())).on(folder_id),
        )
        .map_err(|denied| ImportError::Skill(denied.into()))?;
        let folder = self
            .folders
            .get(folder_id)
            .await?
            .ok_or(SkillError::FolderNotFound)?;
        let source = source_of_folder(&folder)?;
        let previous_commit = folder.source.as_ref().and_then(|s| s.commit.clone());
        let credentials = self.source_credentials(actor, &source);
        let outcome = self.scanner.scan(scan_id, &source, &credentials).await?;
        let kept = self.scanner.kept(scan_id).ok_or(ImportError::ScanExpired)?;
        let skills = self.skills.list().await?;
        let folders = self.folders.list().await?;
        let from_source = from_source(&skills, &source);
        let mut entries = Vec::new();
        let mut seen_paths = HashSet::new();
        for candidate in &outcome.candidates {
            seen_paths.insert(candidate.path.clone());
            let entry = match matched_skill(&from_source, folder_id, &candidate.path) {
                Some(skill) => {
                    let in_folder = skill.folder_id.as_deref() == Some(folder_id);
                    let diff = (in_folder && candidate.valid).then(|| {
                        TreeDiff::between(
                            &self.content.hashes(&skill.id),
                            &self.scanner.hashes(&kept.root.join(&candidate.path)),
                        )
                    });
                    known_entry(candidate, skill, folder_id, &folders, diff)
                }
                None => {
                    let taken = if candidate.valid {
                        self.skills
                            .find_by_name(&candidate.name)
                            .await?
                            .map(|s| s.name)
                    } else {
                        None
                    };
                    new_entry(candidate, taken)
                }
            };
            entries.push(entry);
        }
        for skill in from_source {
            if skill.folder_id.as_deref() != Some(folder_id) {
                continue;
            }
            let path = skill
                .source
                .as_ref()
                .map(|p| p.path.clone())
                .unwrap_or_default();
            if !seen_paths.contains(&path) {
                entries.push(gone_entry(skill));
            }
        }
        Ok(RescanReport {
            scan_id: scan_id.to_string(),
            source,
            previous_commit,
            commit: outcome.commit,
            entries,
        })
    }
}
