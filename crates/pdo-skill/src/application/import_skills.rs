//! Import the checked candidates of a scan into a Source folder (new or given),
//! with the explicit per-collision choice (#670).

use std::path::{Path, PathBuf};

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::import::{
    folder_provenance_for, provenance_for, FailedRow, ImportAction, ImportFolder, ImportItem,
    ImportReport, ImportedRow,
};
use crate::domain::import_error::ImportError;
use crate::domain::skill::{Provenance, Skill};
use crate::domain::skill_md::validate_skill_md;
use crate::domain::source::{parse_source, ParsedSource};

impl Skills {
    /// Per-row failures are reported, not fatal (the popup keeps the rows red
    /// and the successes ticked). The folder receives the source's provenance
    /// at the end.
    pub async fn import_skills(
        &self,
        actor: &Actor,
        scan_id: &str,
        source: &str,
        home: Option<&Path>,
        folder: ImportFolder,
        items: Vec<ImportItem>,
    ) -> Result<ImportReport, ImportError> {
        let source = parse_source(source, home)?;
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Action("import".into())),
        )
        .map_err(|denied| ImportError::Skill(denied.into()))?;
        let (root, commit) = self.resolve_scan(actor, scan_id, &source).await?;
        let candidates = self.scanner.candidates(&root, &source.path);
        // Refuse before any write: every non-skip row must be resolvable.
        for item in &items {
            if item.action == ImportAction::Skip {
                continue;
            }
            self.scanner.candidate_dir(&root, &item.path)?;
            if item.unresolved() {
                return Err(ImportError::UnresolvedCollision(item.path.clone()));
            }
        }
        let (target, created) = match folder.id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => (
                self.folders
                    .get(id)
                    .await?
                    .ok_or(SkillError::FolderNotFound)?,
                false,
            ),
            None => {
                let name = folder
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&source.suggested_folder);
                (
                    self.new_folder(name, folder.parent_id.as_deref()).await?,
                    true,
                )
            }
        };
        let mut imported = Vec::new();
        let mut failures = Vec::new();
        for item in items {
            if item.action == ImportAction::Skip {
                continue;
            }
            let dir = match self.scanner.candidate_dir(&root, &item.path) {
                Ok(dir) => dir,
                Err(error) => {
                    failures.push(FailedRow::new(&item.path, error));
                    continue;
                }
            };
            let provenance = provenance_for(&source, commit.as_deref(), &item.path);
            match self.import_one(&item, &dir, &target.id, &provenance).await {
                Ok((skill, action)) => imported.push(ImportedRow {
                    path: item.path,
                    skill,
                    action,
                }),
                Err(error) => failures.push(FailedRow::new(&item.path, error)),
            }
        }
        if imported.is_empty() && created {
            // Nothing landed: do not leave an empty Source folder behind.
            let _ = self.remove_folder(&target.id).await;
            return Ok(ImportReport {
                folder: target,
                imported,
                failed: failures,
                commit,
            });
        }
        let folder = self
            .stamp_source_folder(&target.id, &source, commit.as_deref(), &candidates)
            .await?;
        self.remember(&source).await;
        Ok(ImportReport {
            folder,
            imported,
            failed: failures,
            commit,
        })
    }

    async fn import_one(
        &self,
        item: &ImportItem,
        dir: &Path,
        folder_id: &str,
        provenance: &Provenance,
    ) -> Result<(Skill, &'static str), ImportError> {
        match item.action {
            ImportAction::Import => Ok((
                self.index_from_dir(dir, None, Some(folder_id), provenance)
                    .await?,
                "imported",
            )),
            ImportAction::Rename => Ok((
                self.index_from_dir(
                    dir,
                    item.name.as_deref().map(str::trim),
                    Some(folder_id),
                    provenance,
                )
                .await?,
                "renamed",
            )),
            ImportAction::Replace => {
                let content = self.scanner.read_skill_md(dir)?;
                let parsed = validate_skill_md(&content)?;
                match self.skills.find_by_name(&parsed.name).await? {
                    None => Ok((
                        self.index_from_dir(dir, None, Some(folder_id), provenance)
                            .await?,
                        "imported",
                    )),
                    Some(existing) => Ok((
                        self.replace_from_dir(&existing.id, dir, provenance).await?,
                        "replaced",
                    )),
                }
            }
            ImportAction::Skip => unreachable!("skipped rows are filtered out"),
        }
    }

    /// The checkout a write refers to: the kept clone, or a fresh scan of the
    /// same source for `actor` when it expired or was cloned with other
    /// credentials than `actor`'s.
    pub(crate) async fn resolve_scan(
        &self,
        actor: &Actor,
        scan_id: &str,
        source: &ParsedSource,
    ) -> Result<(PathBuf, Option<String>), ImportError> {
        let credentials = self.source_credentials(actor, source);
        if let Some(kept) = self.scanner.kept(scan_id) {
            if kept.source.url == source.url
                && kept.source.git_ref == source.git_ref
                && kept.credentials == credentials.fingerprint()
            {
                return Ok((kept.root, kept.commit));
            }
        }
        let outcome = self.scanner.scan(scan_id, source, &credentials).await?;
        let kept = self.scanner.kept(scan_id).ok_or(ImportError::ScanExpired)?;
        Ok((kept.root, outcome.commit))
    }

    /// Stamp (or refresh) a Source folder's provenance from what the source
    /// holds now.
    pub(crate) async fn stamp_source_folder(
        &self,
        folder_id: &str,
        source: &ParsedSource,
        commit: Option<&str>,
        candidates: &[crate::domain::import::Candidate],
    ) -> Result<crate::domain::skill::SkillFolder, SkillError> {
        self.folders
            .get(folder_id)
            .await?
            .ok_or(SkillError::FolderNotFound)?;
        let provenance = folder_provenance_for(source, commit, candidates, now_iso());
        self.folders
            .set_provenance(folder_id, &provenance, &now_iso())
            .await?;
        self.folders
            .get(folder_id)
            .await?
            .ok_or(SkillError::FolderNotFound)
    }
}
