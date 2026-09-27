//! The reads of the skill context: straight to the ports, no authorization.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::ports::Referents;
use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::file_path::reference_file;
use crate::domain::selection::SkillRef;
use crate::domain::sidecar::SidecarSkill;
use crate::domain::skill::{
    is_safe_skill_id, RecentSource, Skill, SkillFile, SkillFolder, SKILL_MD,
};
use crate::domain::skill_md::{validate_skill_md, ParsedSkillMd};

/// How many recent sources the import popup lists.
const RECENT_SOURCES: usize = 12;

/// The whole bank in one read.
#[derive(Debug, Clone)]
pub struct BankListing {
    pub skills: Vec<Skill>,
    pub folders: Vec<SkillFolder>,
    /// Where the skill folders live on disk.
    pub root_path: PathBuf,
}

/// A skill's row plus its content. A skill whose folder vanished from disk
/// still answers with its row and no content: the row is the truth of the
/// index.
#[derive(Debug, Clone)]
pub struct SkillDetail {
    pub skill: Skill,
    pub content: Option<String>,
    /// `None` when the content is absent or no longer passes the gate.
    pub parsed: Option<ParsedSkillMd>,
    pub files: Vec<SkillFile>,
    pub path: PathBuf,
}

/// One file of a skill for the plain-text editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    pub path: String,
    pub size: u64,
    /// `None` when the bytes are not UTF-8.
    pub text: Option<String>,
}

impl Skills {
    pub async fn bank(&self) -> Result<BankListing, SkillError> {
        Ok(BankListing {
            skills: self.skills.list().await?,
            folders: self.folders.list().await?,
            root_path: self.content.root(),
        })
    }

    pub async fn skill(&self, id: &str) -> Result<Option<Skill>, SkillError> {
        self.skills.get(id).await
    }

    pub async fn skill_detail(&self, id: &str) -> Result<SkillDetail, SkillError> {
        let skill = self.skills.get(id).await?.ok_or(SkillError::NotFound)?;
        let content = self.content.read_skill_md(id).ok();
        let parsed = content
            .as_deref()
            .and_then(|text| validate_skill_md(text).ok());
        Ok(SkillDetail {
            skill,
            content,
            parsed,
            files: self.files(id),
            path: self.content.dir(id),
        })
    }

    /// The reference files of a skill; empty when its folder is unreadable.
    pub fn files(&self, id: &str) -> Vec<SkillFile> {
        self.content.list_files(id).unwrap_or_default()
    }

    /// One file of the skill, `SKILL.md` included: the editor reads it
    /// through the same seam.
    pub async fn read_file(&self, id: &str, path: &str) -> Result<FileContent, SkillError> {
        if self.skills.get(id).await?.is_none() {
            return Err(SkillError::NotFound);
        }
        let rel = reference_file(path, true)?;
        let bytes = self
            .content
            .read_file(id, &rel)?
            .ok_or_else(|| SkillError::FileNotFound(rel.clone()))?;
        Ok(FileContent {
            path: rel,
            size: bytes.len() as u64,
            text: String::from_utf8(bytes).ok(),
        })
    }

    pub async fn folders(&self) -> Result<Vec<SkillFolder>, SkillError> {
        self.folders.list().await
    }

    pub async fn folder(&self, id: &str) -> Result<Option<SkillFolder>, SkillError> {
        self.folders.get(id).await
    }

    /// The sources the operator scanned, most recent first, each with the
    /// Source folder that holds it.
    pub async fn recent_sources(&self) -> Result<Vec<RecentSource>, SkillError> {
        let recent = self.sources.recent(RECENT_SOURCES).await?;
        let folders = self.folders.list().await?;
        Ok(recent
            .into_iter()
            .map(|s| RecentSource::held_by(s.url, s.git_ref, s.path, s.last_used_at, &folders))
            .collect())
    }

    /// Who selects this skill, by tier (#669). Informative, never blocking:
    /// the delete is unconditional, and every referent keeps the id and shows
    /// a warning afterwards.
    pub async fn referents(&self, id: &str) -> Result<Referents, SkillError> {
        if self.skills.get(id).await?.is_none() {
            return Err(SkillError::NotFound);
        }
        Ok(self.referents.referents(id).await)
    }

    /// The `id → name` map of the whole bank, read ONCE per resolution — the
    /// snapshot the selection resolves names from (identity is the id; the
    /// stored label is only a fallback for a deleted one).
    pub async fn bank_names(&self) -> Result<BTreeMap<String, String>, SkillError> {
        self.skills.names().await
    }

    /// The content of every skill in `references`, read from the bank, for a
    /// pipeline document's sidecar. A referenced id with no folder is not in
    /// it; the ids skipped are returned beside the skills.
    pub fn sidecar_of(&self, references: &[SkillRef]) -> (Vec<SidecarSkill>, Vec<String>) {
        let mut skills = Vec::new();
        let mut absent = Vec::new();
        for reference in references {
            if !is_safe_skill_id(&reference.id) {
                absent.push(reference.id.clone());
                continue;
            }
            let Ok(skill_md) = self.content.read_skill_md(&reference.id) else {
                absent.push(reference.id.clone());
                continue;
            };
            let mut files = BTreeMap::new();
            files.insert(SKILL_MD.to_string(), skill_md.into_bytes());
            for file in self.files(&reference.id) {
                if let Ok(Some(data)) = self.content.read_file(&reference.id, &file.path) {
                    files.insert(file.path, data);
                }
            }
            skills.push(SidecarSkill {
                id: reference.id.clone(),
                files,
            });
        }
        (skills, absent)
    }
}
