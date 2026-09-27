//! The content of the bank on disk, and the tree helpers the other adapters
//! share (copy a skill folder, hash it).

mod content;

pub use content::{skill_dir, skills_root, FsSkillContent};

use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::domain::update::TreeHashes;

/// Copy a directory tree (files and sub-directories; symlinks are followed as
/// files, `.git` folders skipped). Used to bring a skill's folder — `SKILL.md`
/// and its reference files, verbatim — from a scanned source into the bank.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// `dir` relative to `root`, `/`-separated.
pub(crate) fn rel_path(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root)
        .unwrap_or(dir)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every file under `dir` (`.git` skipped) with its SHA-256.
pub fn hash_tree(dir: &Path) -> TreeHashes {
    fn walk(base: &Path, dir: &Path, out: &mut HashMap<String, [u8; 32]>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|f| f.to_str()) == Some(".git") {
                    continue;
                }
                walk(base, &path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                out.insert(rel_path(base, &path), hasher.finalize().into());
            }
        }
    }
    let mut out = HashMap::new();
    walk(dir, dir, &mut out);
    out
}
