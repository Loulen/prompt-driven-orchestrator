//! A **bundle** of the bank: skills exported from it as a zip, and the same zip
//! uploaded back into a bank. The archive is the sidecar's layout
//! (`<name>.skills/<id>/SKILL.md` + reference files, see
//! [`super::sidecar`]), so one reader serves the pipeline document and the
//! bank's upload.
//!
//! An export names skills and folders; a folder stands for every skill under
//! it at this instant, sub-folders included (a gesture, as in a selector). The
//! zip's name says what was exported: the folder, the skill, or `skills` for a
//! mixed selection. An upload files the skills it creates under a root folder
//! named after the zip, each labelled with its `SKILL.md` `name`: the zip
//! carries no bank label, so a skill relabelled in the bank comes back under
//! its frontmatter name.

use std::collections::HashSet;

use super::error::SkillError;
use super::folders::is_self_or_descendant;
use super::selection::SkillRef;
use super::sidecar::SIDECAR_SUFFIX;
use super::skill::{Skill, SkillFolder};

/// The name of a mixed selection's zip, and of an upload's folder when the
/// file name says nothing.
pub const DEFAULT_BUNDLE_NAME: &str = "skills";

/// What an export ships: the zip's name and the skills, in bank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleSelection {
    pub name: String,
    pub skills: Vec<SkillRef>,
}

/// Resolve an export request against the bank. An unknown skill or folder id
/// is refused; a selection that ends up with no skill is
/// [`SkillError::NothingToExport`].
pub fn bundle_selection(
    skills: &[Skill],
    folders: &[SkillFolder],
    skill_ids: &[String],
    folder_ids: &[String],
) -> Result<BundleSelection, SkillError> {
    for id in skill_ids {
        if !skills.iter().any(|s| &s.id == id) {
            return Err(SkillError::NotFound);
        }
    }
    for id in folder_ids {
        if !folders.iter().any(|f| &f.id == id) {
            return Err(SkillError::FolderNotFound);
        }
    }
    let named: HashSet<&str> = skill_ids.iter().map(String::as_str).collect();
    let picked = skills
        .iter()
        .filter(|skill| {
            named.contains(skill.id.as_str())
                || skill.folder_id.as_deref().is_some_and(|folder| {
                    folder_ids
                        .iter()
                        .any(|root| is_self_or_descendant(folders, root, folder))
                })
        })
        .map(|skill| SkillRef {
            id: skill.id.clone(),
            name: skill.name.clone(),
        })
        .collect::<Vec<_>>();
    if picked.is_empty() {
        return Err(SkillError::NothingToExport);
    }
    let name = match (skill_ids, folder_ids) {
        ([], [folder]) => folders
            .iter()
            .find(|f| &f.id == folder)
            .map(|f| f.name.clone()),
        ([_], []) => picked.first().map(|s| s.name.clone()),
        _ => None,
    }
    .unwrap_or_else(|| DEFAULT_BUNDLE_NAME.to_string());
    Ok(BundleSelection {
        name,
        skills: picked,
    })
}

/// The folder an uploaded zip's new skills go to: its file name without
/// `.zip` and `.skills` (`PDO.skills.zip` → `PDO`), [`DEFAULT_BUNDLE_NAME`]
/// when nothing is left.
pub fn bundle_name_of_file(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let base = strip_suffix_ignore_case(base, ".zip");
    let base = strip_suffix_ignore_case(base, SIDECAR_SUFFIX).trim();
    if base.is_empty() {
        DEFAULT_BUNDLE_NAME.to_string()
    } else {
        base.to_string()
    }
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> &'a str {
    let cut = text.len().saturating_sub(suffix.len());
    match text.get(cut..) {
        Some(tail) if tail.eq_ignore_ascii_case(suffix) => &text[..cut],
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, folder: Option<&str>) -> Skill {
        Skill {
            id: id.into(),
            name: format!("{id}-name"),
            description: "d".into(),
            folder_id: folder.map(Into::into),
            locked: false,
            extension: None,
            source: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn folder(id: &str, parent: Option<&str>) -> SkillFolder {
        SkillFolder {
            id: id.into(),
            name: id.to_uppercase(),
            parent_id: parent.map(Into::into),
            source: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn ids(selection: &BundleSelection) -> Vec<&str> {
        selection.skills.iter().map(|s| s.id.as_str()).collect()
    }

    fn bank() -> (Vec<Skill>, Vec<SkillFolder>) {
        (
            vec![
                skill("a", Some("top")),
                skill("b", Some("sub")),
                skill("c", None),
                skill("d", Some("other")),
            ],
            vec![
                folder("top", None),
                folder("sub", Some("top")),
                folder("other", None),
            ],
        )
    }

    #[test]
    fn a_folder_exports_every_skill_below_it_and_names_the_zip() {
        let (skills, folders) = bank();
        let selection = bundle_selection(&skills, &folders, &[], &["top".into()]).unwrap();
        assert_eq!(ids(&selection), vec!["a", "b"]);
        assert_eq!(selection.name, "TOP");
    }

    #[test]
    fn one_skill_names_the_zip_after_it() {
        let (skills, folders) = bank();
        let selection = bundle_selection(&skills, &folders, &["c".into()], &[]).unwrap();
        assert_eq!(ids(&selection), vec!["c"]);
        assert_eq!(selection.name, "c-name");
    }

    #[test]
    fn a_mixed_selection_is_deduplicated_in_bank_order() {
        let (skills, folders) = bank();
        let selection =
            bundle_selection(&skills, &folders, &["d".into(), "b".into()], &["sub".into()])
                .unwrap();
        assert_eq!(ids(&selection), vec!["b", "d"]);
        assert_eq!(selection.name, DEFAULT_BUNDLE_NAME);
    }

    #[test]
    fn unknown_ids_and_empty_selections_are_refused() {
        let (skills, mut folders) = bank();
        assert_eq!(
            bundle_selection(&skills, &folders, &["zz".into()], &[]),
            Err(SkillError::NotFound)
        );
        assert_eq!(
            bundle_selection(&skills, &folders, &[], &["zz".into()]),
            Err(SkillError::FolderNotFound)
        );
        assert_eq!(
            bundle_selection(&skills, &folders, &[], &[]),
            Err(SkillError::NothingToExport)
        );
        folders.push(folder("empty", None));
        assert_eq!(
            bundle_selection(&skills, &folders, &[], &["empty".into()]),
            Err(SkillError::NothingToExport)
        );
    }

    #[test]
    fn an_upload_is_named_after_its_file() {
        assert_eq!(bundle_name_of_file("PDO.skills.zip"), "PDO");
        assert_eq!(bundle_name_of_file("team.ZIP"), "team");
        assert_eq!(bundle_name_of_file("/tmp/x/review.skills.zip"), "review");
        assert_eq!(bundle_name_of_file(".zip"), DEFAULT_BUNDLE_NAME);
        assert_eq!(bundle_name_of_file(""), DEFAULT_BUNDLE_NAME);
    }
}
