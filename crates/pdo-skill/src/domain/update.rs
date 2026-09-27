//! Rescan and update of a **Source folder** (#670): the diff between what a
//! folder imported and what its source holds now.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::import::Candidate;
use super::skill::{folder_name_of, Skill, SkillFolder, SKILL_MD};
use super::source::ParsedSource;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillUpdateStatus")]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    /// Content differs at the source; checked by default.
    Updated,
    /// Identical content; only the provenance commit moves.
    Unchanged,
    /// At the source, not in the bank; unchecked by default.
    New,
    /// Imported from this source but living in another folder (moved out, or
    /// imported again elsewhere): never written to from here.
    Skipped,
    /// In this folder, no longer at the source; kept and flagged.
    Gone,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillUpdateEntry")]
pub struct UpdateEntry {
    pub path: String,
    pub name: String,
    pub description: String,
    pub status: UpdateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    /// `updated` only.
    #[serde(default)]
    pub skill_md_changed: bool,
    #[serde(default)]
    pub files_added: usize,
    #[serde(default)]
    pub files_removed: usize,
    #[serde(default)]
    pub files_changed: usize,
    /// `new` only: another skill already carries this name.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name_taken_by: Option<String>,
}

impl UpdateEntry {
    fn plain(path: &str, name: &str, description: &str, status: UpdateStatus) -> Self {
        UpdateEntry {
            path: path.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            status,
            skill_id: None,
            reason: None,
            skill_md_changed: false,
            files_added: 0,
            files_removed: 0,
            files_changed: 0,
            name_taken_by: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, rename = "SkillRescanReport")]
pub struct RescanReport {
    pub scan_id: String,
    pub source: ParsedSource,
    pub previous_commit: Option<String>,
    pub commit: Option<String>,
    pub entries: Vec<UpdateEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateAction {
    /// Replace the content of the skill matched by path.
    Update,
    /// Import a skill new at the source into this folder.
    Import,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateItem {
    pub path: String,
    pub action: UpdateAction,
}

/// The files of a skill folder, by `/`-separated path, with their SHA-256.
pub type TreeHashes = HashMap<String, [u8; 32]>;

/// How the bank's copy of a skill (`here`) differs from the source (`there`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeDiff {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub skill_md_changed: bool,
}

impl TreeDiff {
    pub fn between(here: &TreeHashes, there: &TreeHashes) -> Self {
        let mut diff = TreeDiff::default();
        for (path, hash) in there {
            match here.get(path) {
                None => diff.added += 1,
                Some(h) if h != hash => {
                    if path == SKILL_MD {
                        diff.skill_md_changed = true;
                    } else {
                        diff.changed += 1;
                    }
                }
                Some(_) => {}
            }
        }
        diff.removed = here.keys().filter(|p| !there.contains_key(*p)).count();
        diff
    }

    pub fn differs(&self) -> bool {
        self.added + self.changed + self.removed > 0 || self.skill_md_changed
    }
}

/// The skills of the bank imported from `source`.
pub fn from_source<'a>(skills: &'a [Skill], source: &ParsedSource) -> Vec<&'a Skill> {
    skills
        .iter()
        .filter(|s| s.source.as_ref().is_some_and(|p| p.url == source.url))
        .collect()
}

/// The skill a candidate path matches. Two folders may come from the same
/// source (a re-import): the skill of *this* folder wins; another folder's
/// skill only yields `skipped`.
pub fn matched_skill<'a>(
    from_source: &[&'a Skill],
    folder_id: &str,
    path: &str,
) -> Option<&'a Skill> {
    let at_path = |s: &&&Skill| s.source.as_ref().is_some_and(|p| p.path == path);
    let in_folder = |s: &&&Skill| s.folder_id.as_deref() == Some(folder_id);
    from_source
        .iter()
        .find(|s| in_folder(s) && at_path(s))
        .or_else(|| from_source.iter().find(at_path))
        .copied()
}

/// What the rescan says of a candidate the folder already holds (`skill`, in
/// `folder_id`) or that another folder holds. `diff` is computed only for a
/// valid candidate the folder holds.
pub fn known_entry(
    candidate: &Candidate,
    skill: &Skill,
    folder_id: &str,
    folders: &[SkillFolder],
    diff: Option<TreeDiff>,
) -> UpdateEntry {
    if skill.folder_id.as_deref() != Some(folder_id) {
        let mut entry = UpdateEntry::plain(
            &candidate.path,
            &skill.name,
            &candidate.description,
            UpdateStatus::Skipped,
        );
        entry.skill_id = Some(skill.id.clone());
        entry.reason = Some(match folder_name_of(folders, skill.folder_id.as_deref()) {
            Some(name) => format!("already in \u{201c}{name}\u{201d}"),
            None => "already in the bank, outside any folder".to_string(),
        });
        return entry;
    }
    let Some(diff) = diff.filter(|_| candidate.valid) else {
        let mut entry = UpdateEntry::plain(
            &candidate.path,
            &skill.name,
            &skill.description,
            UpdateStatus::Invalid,
        );
        entry.skill_id = Some(skill.id.clone());
        entry.reason = candidate.reason.clone();
        return entry;
    };
    let status = if diff.differs() {
        UpdateStatus::Updated
    } else {
        UpdateStatus::Unchanged
    };
    let mut entry =
        UpdateEntry::plain(&candidate.path, &skill.name, &candidate.description, status);
    entry.skill_id = Some(skill.id.clone());
    entry.skill_md_changed = diff.skill_md_changed;
    entry.files_added = diff.added;
    entry.files_removed = diff.removed;
    entry.files_changed = diff.changed;
    entry
}

/// What the rescan says of a candidate no skill of the bank came from.
/// `name_taken_by` is the label of the skill already carrying its name.
pub fn new_entry(candidate: &Candidate, name_taken_by: Option<String>) -> UpdateEntry {
    if !candidate.valid {
        let mut entry =
            UpdateEntry::plain(&candidate.path, &candidate.name, "", UpdateStatus::Invalid);
        entry.reason = candidate.reason.clone();
        return entry;
    }
    let mut entry = UpdateEntry::plain(
        &candidate.path,
        &candidate.name,
        &candidate.description,
        UpdateStatus::New,
    );
    entry.files_added = candidate.file_count;
    entry.name_taken_by = name_taken_by;
    entry
}

/// A skill of this folder its source no longer holds: kept, and flagged.
pub fn gone_entry(skill: &Skill) -> UpdateEntry {
    let path = skill
        .source
        .as_ref()
        .map(|p| p.path.clone())
        .unwrap_or_default();
    let mut entry = UpdateEntry::plain(&path, &skill.name, &skill.description, UpdateStatus::Gone);
    entry.skill_id = Some(skill.id.clone());
    entry.reason = Some("no longer at the source · kept in the bank".to_string());
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(entries: &[(&str, u8)]) -> TreeHashes {
        entries
            .iter()
            .map(|(p, b)| (p.to_string(), [*b; 32]))
            .collect()
    }

    #[test]
    fn a_tree_diff_counts_each_kind_of_change() {
        let here = hashes(&[("SKILL.md", 1), ("a.md", 1), ("gone.md", 1)]);
        let there = hashes(&[("SKILL.md", 2), ("a.md", 3), ("new.md", 1)]);
        let diff = TreeDiff::between(&here, &there);
        assert_eq!(
            diff,
            TreeDiff {
                added: 1,
                removed: 1,
                changed: 1,
                skill_md_changed: true
            }
        );
        assert!(diff.differs());
        assert!(!TreeDiff::between(&here, &here).differs());
    }
}
