//! The [`SkillContent`] port over the local filesystem: one folder per skill
//! under `<repo_root>/.pdo/skills/<id>/` — the same `.pdo/` the SQLite index
//! lives in, so content and index travel together.

use std::path::{Path, PathBuf};

use super::{copy_dir_all, hash_tree, rel_path};
use crate::application::ports::{SkillContent, StoreResult};
use crate::domain::error::SkillError;
use crate::domain::skill::{SkillFile, SKILL_MD};
use crate::domain::update::TreeHashes;

/// The on-disk root of the bank, relative to the daemon's repo root.
pub fn skills_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".pdo").join("skills")
}

/// Where one skill's folder lives. Keyed by id, never by name.
pub fn skill_dir(repo_root: &Path, id: &str) -> PathBuf {
    skills_root(repo_root).join(id)
}

pub struct FsSkillContent {
    repo_root: PathBuf,
}

impl FsSkillContent {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
        }
    }

    /// `rel` (already normalised) under the folder of `id`.
    fn file(&self, id: &str, rel: &str) -> PathBuf {
        let mut path = self.dir(id);
        for segment in rel.split('/') {
            path.push(segment);
        }
        path
    }
}

impl SkillContent for FsSkillContent {
    fn root(&self) -> PathBuf {
        skills_root(&self.repo_root)
    }

    fn dir(&self, id: &str) -> PathBuf {
        skill_dir(&self.repo_root, id)
    }

    fn read_skill_md(&self, id: &str) -> StoreResult<String> {
        Ok(std::fs::read_to_string(self.dir(id).join(SKILL_MD))?)
    }

    fn write_skill_md(&self, id: &str, content: &str) -> StoreResult<()> {
        let dir = self.dir(id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(SKILL_MD), content)?;
        Ok(())
    }

    fn list_files(&self, id: &str) -> StoreResult<Vec<SkillFile>> {
        let root = self.dir(id);
        let mut out = Vec::new();
        fn walk(base: &Path, dir: &Path, out: &mut Vec<SkillFile>) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                let meta = entry.metadata()?;
                if meta.is_dir() {
                    walk(base, &path, out)?;
                } else {
                    let rel = rel_path(base, &path);
                    if rel == SKILL_MD {
                        continue;
                    }
                    out.push(SkillFile {
                        path: rel,
                        size: meta.len(),
                    });
                }
            }
            Ok(())
        }
        if root.is_dir() {
            walk(&root, &root, &mut out)?;
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn read_file(&self, id: &str, rel: &str) -> StoreResult<Option<Vec<u8>>> {
        let path = self.file(id, rel);
        if !path.is_file() {
            return Ok(None);
        }
        Ok(Some(std::fs::read(&path)?))
    }

    fn is_file(&self, id: &str, rel: &str) -> bool {
        self.file(id, rel).is_file()
    }

    fn write_file(&self, id: &str, rel: &str, data: &[u8]) -> StoreResult<()> {
        let path = self.file(id, rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        Ok(())
    }

    fn delete_file(&self, id: &str, rel: &str) -> StoreResult<()> {
        let path = self.file(id, rel);
        std::fs::remove_file(&path)?;
        let root = self.dir(id);
        let mut cursor = path.parent().map(Path::to_path_buf);
        while let Some(dir) = cursor {
            if dir == root || !dir.starts_with(&root) {
                break;
            }
            if std::fs::remove_dir(&dir).is_err() {
                break;
            }
            cursor = dir.parent().map(Path::to_path_buf);
        }
        Ok(())
    }

    fn remove(&self, id: &str) -> StoreResult<()> {
        let dir = self.dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    fn copy_from(&self, id: &str, src: &Path) -> StoreResult<()> {
        let dir = self.dir(id);
        if let Err(error) = copy_dir_all(src, &dir) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(SkillError::Storage(format!(
                "failed to copy {} into {}: {error}",
                src.display(),
                dir.display()
            )));
        }
        Ok(())
    }

    fn replace_from(&self, id: &str, src: &Path) -> StoreResult<()> {
        let dir = self.dir(id);
        let backup = self.root().join(format!(".{id}.replacing"));
        let _ = std::fs::remove_dir_all(&backup);
        if dir.exists() {
            std::fs::rename(&dir, &backup)?;
        }
        if let Err(error) = copy_dir_all(src, &dir) {
            let _ = std::fs::remove_dir_all(&dir);
            if backup.exists() {
                let _ = std::fs::rename(&backup, &dir);
            }
            return Err(SkillError::Storage(format!(
                "failed to copy {} into {}: {error}",
                src.display(),
                dir.display()
            )));
        }
        let _ = std::fs::remove_dir_all(&backup);
        Ok(())
    }

    fn hashes(&self, id: &str) -> TreeHashes {
        hash_tree(&self.dir(id))
    }

    fn host_file_size(&self, path: &Path) -> Option<u64> {
        std::fs::metadata(path)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
    }

    fn read_host_file(&self, path: &Path) -> StoreResult<Vec<u8>> {
        Ok(std::fs::read(path)?)
    }
}
