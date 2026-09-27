//! Add reference files to a skill (#671 design "deux chemins d'upload"): files
//! the browser sends, or a file the explorer picked on the daemon's host.

use std::path::Path;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::{SkillError, MAX_FILE_BYTES};
use crate::domain::file_path::{normalise_file_path, reference_file};
use crate::domain::seed::refuse_locked;
use crate::domain::skill::SkillFile;

/// One file to write, under its path relative to the skill's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub path: String,
    pub data: Vec<u8>,
}

/// What an upload wrote before it stopped, and why it stopped if it did.
#[derive(Debug, Default)]
pub struct Uploaded {
    pub uploaded: Vec<SkillFile>,
    pub failure: Option<SkillError>,
}

impl Skills {
    fn edit_files(&self, actor: &Actor, id: &str) -> Result<(), SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Update).on(id),
        )?;
        Ok(())
    }

    /// Write the files one by one, in order; the first refusal stops the batch
    /// and is returned beside the files already written, so the popup can mark
    /// them and retry the rest. A `SKILL.md` is refused here
    /// ([`SkillError::SkillMdReserved`]): the editor saves it instead.
    pub async fn upload_files(&self, actor: &Actor, id: &str, uploads: Vec<Upload>) -> Uploaded {
        let mut outcome = Uploaded::default();
        if let Err(error) = self.edit_files(actor, id) {
            outcome.failure = Some(error);
            return outcome;
        }
        for upload in uploads {
            match self.write_reference_file(id, &upload.path, &upload.data) {
                Ok(file) => outcome.uploaded.push(file),
                Err(error) => {
                    outcome.failure = Some(error);
                    break;
                }
            }
        }
        outcome
    }

    /// Copy a file the explorer picked (an absolute path on the daemon's host)
    /// into the skill folder, under `rel` (defaults to the source file name).
    pub async fn copy_host_file(
        &self,
        actor: &Actor,
        id: &str,
        from: &Path,
        rel: Option<&str>,
    ) -> Result<SkillFile, SkillError> {
        self.edit_files(actor, id)?;
        if !from.is_absolute() {
            return Err(SkillError::SourceNotAFile(from.display().to_string()));
        }
        let size = self
            .content
            .host_file_size(from)
            .ok_or_else(|| SkillError::SourceNotAFile(from.display().to_string()))?;
        let rel = match rel {
            Some(rel) => rel.to_string(),
            None => from
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| SkillError::SourceNotAFile(from.display().to_string()))?,
        };
        if size > MAX_FILE_BYTES {
            return Err(SkillError::FileTooLarge {
                path: normalise_file_path(&rel)?,
                size,
            });
        }
        let data = self.content.read_host_file(from)?;
        self.write_reference_file(id, &rel, &data)
    }

    /// Write (create or replace) a reference file from bytes. The skill folder
    /// exists (the row was indexed first); intermediate sub-folders are made.
    pub(crate) fn write_reference_file(
        &self,
        id: &str,
        rel: &str,
        data: &[u8],
    ) -> Result<SkillFile, SkillError> {
        refuse_locked(id)?;
        let rel = reference_file(rel, false)?;
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(SkillError::FileTooLarge {
                path: rel,
                size: data.len() as u64,
            });
        }
        self.content.write_file(id, &rel, data)?;
        Ok(SkillFile {
            path: rel,
            size: data.len() as u64,
        })
    }
}
