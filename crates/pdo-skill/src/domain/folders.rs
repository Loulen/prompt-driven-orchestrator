//! The bank's free folder hierarchy. A folder is a UI gesture: nothing
//! references it, so names are free (only blank is refused) and deleting one
//! never deletes a skill (design decision of the #668 mock-up).

use super::error::SkillError;
use super::skill::SkillFolder;

/// A folder name: trimmed, never blank. Two sibling folders may share one.
pub fn folder_name(name: &str) -> Result<String, SkillError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SkillError::EmptyFolderName);
    }
    Ok(name.to_string())
}

/// A folder id sent in a body: blank means "the root".
pub fn folder_ref(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|s| !s.is_empty())
}

/// Is `candidate` equal to `folder` or one of its descendants in `folders`?
/// Guards a move against creating a cycle. A broken chain longer than the
/// hierarchy counts as a cycle.
pub fn is_self_or_descendant(folders: &[SkillFolder], folder: &str, candidate: &str) -> bool {
    let mut cursor = Some(candidate.to_string());
    let mut hops = 0;
    while let Some(current) = cursor {
        if current == folder {
            return true;
        }
        hops += 1;
        if hops > 1000 {
            return true;
        }
        cursor = folders
            .iter()
            .find(|f| f.id == current)
            .and_then(|f| f.parent_id.clone());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, parent: Option<&str>) -> SkillFolder {
        SkillFolder {
            id: id.into(),
            name: id.into(),
            parent_id: parent.map(str::to_string),
            source: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn a_folder_cannot_move_under_itself_or_a_descendant() {
        let folders = [
            folder("a", None),
            folder("b", Some("a")),
            folder("c", Some("b")),
        ];
        assert!(is_self_or_descendant(&folders, "a", "a"));
        assert!(is_self_or_descendant(&folders, "a", "c"));
        assert!(!is_self_or_descendant(&folders, "c", "a"));
        assert!(!is_self_or_descendant(&folders, "b", "unknown"));
    }

    #[test]
    fn names_are_trimmed_and_never_blank() {
        assert_eq!(folder_name(" acme ").unwrap(), "acme");
        assert_eq!(folder_name("  ").unwrap_err(), SkillError::EmptyFolderName);
        assert_eq!(folder_ref(Some("  ")), None);
        assert_eq!(folder_ref(Some(" skf-1 ")), Some("skf-1"));
    }
}
