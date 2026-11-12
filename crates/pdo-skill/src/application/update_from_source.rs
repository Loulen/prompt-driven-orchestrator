//! Apply an Update from source after its rescan: replace the checked skills'
//! content, import the checked new ones, move every unchanged skill's commit
//! forward, and refresh the folder's provenance.

use std::collections::{HashMap, HashSet};

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::import::{provenance_for, FailedRow, ImportReport, ImportedRow};
use crate::domain::import_error::ImportError;
use crate::domain::skill::Skill;
use crate::domain::source::source_of_folder;
use crate::domain::update::{UpdateAction, UpdateItem};

impl Skills {
    pub async fn update_from_source(
        &self,
        actor: &Actor,
        folder_id: &str,
        scan_id: &str,
        items: Vec<UpdateItem>,
    ) -> Result<ImportReport, ImportError> {
        self.authorize(
            actor,
            Operation::new(
                ResourceKind::SkillFolder,
                Verb::Action("update_from_source".into()),
            )
            .on(folder_id),
        )
        .map_err(|denied| ImportError::Skill(denied.into()))?;
        let folder = self
            .folders
            .get(folder_id)
            .await?
            .ok_or(SkillError::FolderNotFound)?;
        let source = source_of_folder(&folder)?;
        let (root, commit) = self.resolve_scan(actor, scan_id, &source).await?;
        let candidates = self.scanner.candidates(&root, &source.path);
        let skills = self.skills.list().await?;
        // Only this folder's skills are ever written to: a sibling folder
        // imported from the same source keeps its own copies untouched.
        let by_path: HashMap<String, &Skill> = skills
            .iter()
            .filter(|s| s.folder_id.as_deref() == Some(folder_id))
            .filter_map(|s| {
                s.source
                    .as_ref()
                    .filter(|p| p.url == source.url)
                    .map(|p| (p.path.clone(), s))
            })
            .collect();
        let mut imported = Vec::new();
        let mut failures = Vec::new();
        let mut touched = HashSet::new();
        for item in items {
            let dir = match self.scanner.candidate_dir(&root, &item.path) {
                Ok(dir) => dir,
                Err(error) => {
                    failures.push(FailedRow::new(&item.path, error));
                    continue;
                }
            };
            let provenance = provenance_for(&source, commit.as_deref(), &item.path);
            let result: Result<(Skill, &'static str), ImportError> = match item.action {
                UpdateAction::Update => match by_path.get(&item.path) {
                    Some(skill) => {
                        touched.insert(skill.id.clone());
                        self.replace_from_dir(&skill.id, &dir, &provenance)
                            .await
                            .map(|s| (s, "updated"))
                            .map_err(Into::into)
                    }
                    None => Err(ImportError::UnknownCandidate(item.path.clone())),
                },
                UpdateAction::Import => self
                    .index_from_dir(&dir, None, Some(folder_id), &provenance)
                    .await
                    .map(|s| (s, "imported"))
                    .map_err(Into::into),
            };
            match result {
                Ok((skill, action)) => imported.push(ImportedRow {
                    path: item.path,
                    skill,
                    action,
                }),
                Err(error) => failures.push(FailedRow::new(&item.path, error)),
            }
        }
        // Skills still identical at the new commit: only their provenance moves.
        for candidate in &candidates {
            let Some(skill) = by_path.get(&candidate.path) else {
                continue;
            };
            if touched.contains(&skill.id) || !candidate.valid {
                continue;
            }
            let here = self.content.hashes(&skill.id);
            let there = self.scanner.hashes(&root.join(&candidate.path));
            if here == there {
                let provenance = provenance_for(&source, commit.as_deref(), &candidate.path);
                let _ = self.skills.set_provenance(&skill.id, &provenance).await;
            }
        }
        let folder = self
            .stamp_source_folder(folder_id, &source, commit.as_deref(), &candidates)
            .await?;
        self.remember(&source).await;
        Ok(ImportReport {
            folder,
            imported,
            failed: failures,
            commit,
        })
    }
}
