//! The **Banque de skills** (#668, spec #667, ADR-0062, CONTEXT.md §*Banque de
//! skills*): the instance-scoped store of Skills PDO delivers into every worktree
//! it creates.
//!
//! Two halves, one seam:
//!
//! - **Content on disk**, one folder per skill holding the `SKILL.md` and its
//!   reference files (#671: uploaded, edited as plain text, deleted — always
//!   inside the skill's folder, never outside it). The folder is keyed by the
//!   **stable id**, never by the name — renaming a skill touches a row, not a
//!   path (#668 AC "renommer ne déplace rien").
//! - **Index**: skills (id, name unique case-insensitively, description, folder,
//!   provenance, dates) and folders (a free hierarchy). The folder is a UI
//!   gesture, not a reference: no tier ever stores a folder id (ADR-0062
//!   "Dossier = geste, pas référence").
//!
//! Identity is the `id`; the `name` is a **label** the bank keeps unique so a
//! selector never shows two indistinguishable rows. Same discipline as the
//! agentic profiles (ADR-0057 ¶2/¶5), and the same reason the uniqueness check
//! lives in code rather than in a bare `UNIQUE` index: the 409 must **name** the
//! clash.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The skill's text inside its folder.
pub const SKILL_MD: &str = "SKILL.md";

/// Where an imported skill comes from (#670, CONTEXT.md §*Source*): the
/// repository URL (or local folder), the ref that was asked for, the commit the
/// content was read at, and the skill's folder path inside the source. Carried
/// by the skill itself — a skill moved out of its Source folder keeps it — and,
/// with a few counters, by the folder the import created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillProvenance")]
pub struct Provenance {
    pub url: String,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    /// `/`-separated path of the skill's folder inside the source (`""` = root).
    #[serde(default)]
    pub path: String,
}

/// A Source folder's provenance: the scan root (`path` is the sub-folder the
/// scan was pointed at), plus what the last import / update saw there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderProvenance {
    pub url: String,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub path: String,
    /// When the folder was last imported or updated from the source.
    pub imported_at: String,
    /// Skills found at the source at that time (valid or not).
    pub found: i64,
    /// Of which not importable (invalid frontmatter).
    pub invalid: i64,
}

/// The extension a skill comes with (ADR-0081, #33): its id and the version
/// the skill was seeded at. The skill is locked, and rewritten at startup when
/// the extension's version or the skill's content changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillExtension")]
pub struct ProvidedBy {
    pub id: String,
    pub version: String,
}

/// One row of the bank's index. `folder_id` is `None` at the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Set on the skills PDO seeds at startup (#722), its own and its
    /// extensions': locked against edit and delete, everywhere the bank is
    /// written.
    #[serde(default)]
    pub locked: bool,
    /// The extension that provides this skill. `None` for every other skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extension: Option<ProvidedBy>,
    /// Provenance of an import. `None` for a pasted skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<Provenance>,
    pub created_at: String,
    pub updated_at: String,
}

/// A folder of the bank's free hierarchy. `parent_id` is `None` at the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillFolder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Set on a folder created by an import (a **Source folder**).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<FolderProvenance>,
    pub created_at: String,
    pub updated_at: String,
}

/// A reference file of a skill (anything under its folder except `SKILL.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillFile {
    /// Path relative to the skill's folder, `/`-separated.
    pub path: String,
    pub size: u64,
}

/// A remembered source, most recent first, with the Source folder that holds
/// it in the bank (if any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "RecentSkillSource")]
pub struct RecentSource {
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub path: String,
    pub last_used_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder_name: Option<String>,
}

impl RecentSource {
    /// A scanned source as the history stores it (`git_ref` empty = none),
    /// named after the Source folder of `folders` that holds it, if any.
    pub fn held_by(
        url: String,
        git_ref: String,
        path: String,
        last_used_at: String,
        folders: &[SkillFolder],
    ) -> Self {
        let holder = folders.iter().find(|folder| {
            folder
                .source
                .as_ref()
                .is_some_and(|s| s.url == url && s.path == path)
        });
        RecentSource {
            url,
            git_ref: if git_ref.is_empty() {
                None
            } else {
                Some(git_ref)
            },
            path,
            last_used_at,
            folder_id: holder.map(|f| f.id.clone()),
            folder_name: holder.map(|f| f.name.clone()),
        }
    }
}

/// A skill id: a UUID, because the folder on disk carries it and the id travels
/// in pipeline documents across instances (ADR-0062 "voyage par document").
pub fn new_skill_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn new_folder_id() -> String {
    format!("skf-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// A skill id that can name a folder on disk: plain characters only, no path
/// separators, no `.`/`..`. The bank's own ids are UUIDs; an imported document
/// (#673) supplies its own, and this is the one gate before it reaches the
/// disk.
pub fn is_safe_skill_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The name of the folder of `folders` whose id is `id`.
pub fn folder_name_of(folders: &[SkillFolder], id: Option<&str>) -> Option<String> {
    id.and_then(|id| folders.iter().find(|f| f.id == id))
        .map(|f| f.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_ids_name_a_folder_and_nothing_else() {
        assert!(is_safe_skill_id("0b7c-uuid_like.1"));
        for bad in ["", ".", "..", "a/b", "a\\b", "a b"] {
            assert!(!is_safe_skill_id(bad), "{bad:?}");
        }
        assert!(!is_safe_skill_id(&"x".repeat(129)));
    }

    #[test]
    fn a_recent_source_names_the_folder_holding_it() {
        let folder = SkillFolder {
            id: "skf-1".into(),
            name: "anthropics/skills".into(),
            parent_id: None,
            source: Some(FolderProvenance {
                url: "u".into(),
                git_ref: None,
                commit: None,
                path: "p".into(),
                imported_at: String::new(),
                found: 0,
                invalid: 0,
            }),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let held = RecentSource::held_by("u".into(), "".into(), "p".into(), "t".into(), &[folder]);
        assert_eq!(held.git_ref, None);
        assert_eq!(held.folder_name.as_deref(), Some("anthropics/skills"));
        let free = RecentSource::held_by("u".into(), "main".into(), "q".into(), "t".into(), &[]);
        assert_eq!(free.git_ref.as_deref(), Some("main"));
        assert_eq!(free.folder_id, None);
    }
}
