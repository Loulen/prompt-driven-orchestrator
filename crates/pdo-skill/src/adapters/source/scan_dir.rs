//! Walk a checkout for the folders holding a `SKILL.md`.

use std::path::{Path, PathBuf};

use crate::adapters::fs::rel_path;
use crate::domain::import::Candidate;
use crate::domain::skill::SKILL_MD;
use crate::domain::skill_md::validate_skill_md;

/// Reference files of a skill folder: everything but its top-level `SKILL.md`
/// (same rule as the bank's file listing).
fn count_files(dir: &Path) -> usize {
    fn walk(dir: &Path, top: bool, n: &mut usize) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, false, n);
                } else if !(top && path.file_name().and_then(|f| f.to_str()) == Some(SKILL_MD)) {
                    *n += 1;
                }
            }
        }
    }
    let mut n = 0;
    walk(dir, true, &mut n);
    n
}

/// Walk `root` recursively for folders holding a `SKILL.md`. A skill folder is
/// not descended into (its sub-folders are its reference files); `.git` is
/// skipped; hidden folders are **not** (skills live under `.agents/skills/`).
pub fn scan_dir(root: &Path, sub_path: &str) -> Vec<Candidate> {
    let start = if sub_path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(sub_path)
    };
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<Candidate>, depth: usize) {
        if depth > 32 {
            return;
        }
        let skill_md = dir.join(SKILL_MD);
        if skill_md.is_file() {
            let path = rel_path(root, dir);
            let folder_name = dir
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let candidate = match validate_skill_md(&content) {
                Ok(parsed) => Candidate {
                    path,
                    name: parsed.name,
                    description: parsed.description,
                    valid: true,
                    reason: None,
                    code: None,
                    file_count: count_files(dir),
                },
                Err(error) => Candidate {
                    path,
                    name: folder_name,
                    description: String::new(),
                    valid: false,
                    reason: Some(error.to_string()),
                    code: Some(error.validation_code().to_string()),
                    file_count: count_files(dir),
                },
            };
            out.push(candidate);
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().and_then(|f| f.to_str()) != Some(".git"))
            .collect();
        children.sort();
        for child in children {
            walk(root, &child, out, depth + 1);
        }
    }
    if start.is_dir() {
        walk(root, &start, &mut out, 0);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}
