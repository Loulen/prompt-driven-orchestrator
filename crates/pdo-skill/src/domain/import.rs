//! Import skills from a **Source** into the Banque de skills (#670, spec #667,
//! CONTEXT.md §*Source*).
//!
//! A scan walks every folder holding a `SKILL.md`, validates each one with the
//! bank's single gate and reports the candidates with their validity and their
//! collisions against the bank. **Nothing is written** before the import call,
//! which names the candidates to take and, per name collision, the explicit
//! choice (replace / rename / skip).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::import_error::ImportError;
use super::skill::{folder_name_of, FolderProvenance, Provenance, Skill, SkillFolder};
use super::source::ParsedSource;

/// One folder holding a `SKILL.md` at the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillCandidateBase")]
pub struct Candidate {
    /// `/`-separated path of the folder from the repository root.
    pub path: String,
    /// Frontmatter `name` when valid, the folder name otherwise.
    pub name: String,
    pub description: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
    /// Reference files besides `SKILL.md`.
    pub file_count: usize,
}

/// Everything a scan reports, before collisions are computed against the bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    pub source: ParsedSource,
    pub commit: Option<String>,
    pub candidates: Vec<Candidate>,
    /// When the sub-path yielded nothing: the folders elsewhere in the
    /// repository that do hold skills (distinct parents), for the empty state.
    pub elsewhere: Vec<String>,
    pub elsewhere_count: usize,
}

/// The distinct parent folders of `all`, for a sub-path that held nothing.
pub fn parents_of(all: &[Candidate]) -> Vec<String> {
    let mut parents: Vec<String> = all
        .iter()
        .map(|c| {
            c.path
                .rsplit_once('/')
                .map(|(p, _)| p.to_string())
                .unwrap_or_default()
        })
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillCandidateStatus")]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    New,
    /// Another skill carries this name: replace / rename / skip.
    NameTaken,
    /// A skill imported from this very source, path and commit already exists.
    SameCommit,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillExistingRef")]
pub struct ExistingRef {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub folder_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillCandidate")]
pub struct ScannedCandidate {
    #[serde(flatten)]
    pub candidate: Candidate,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub existing: Option<ExistingRef>,
}

/// A candidate against the bank: `existing` is the skill already carrying its
/// name (case-insensitively), if any.
pub fn classify(
    candidate: Candidate,
    existing: Option<&Skill>,
    source: &ParsedSource,
    commit: Option<&str>,
    folders: &[SkillFolder],
) -> ScannedCandidate {
    if !candidate.valid {
        return ScannedCandidate {
            candidate,
            status: CandidateStatus::Invalid,
            existing: None,
        };
    }
    let Some(skill) = existing else {
        return ScannedCandidate {
            candidate,
            status: CandidateStatus::New,
            existing: None,
        };
    };
    let same = skill.source.as_ref().is_some_and(|p| {
        p.url == source.url
            && p.path == candidate.path
            && commit.is_some()
            && p.commit.as_deref() == commit
    });
    ScannedCandidate {
        candidate,
        status: if same {
            CandidateStatus::SameCommit
        } else {
            CandidateStatus::NameTaken
        },
        existing: Some(ExistingRef {
            id: skill.id.clone(),
            name: skill.name.clone(),
            folder_name: folder_name_of(folders, skill.folder_id.as_deref()),
            folder_id: skill.folder_id.clone(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, rename = "SkillImportAction")]
#[serde(rename_all = "snake_case")]
pub enum ImportAction {
    /// Take it as a new skill (status `new`).
    Import,
    /// Overwrite the content of the existing skill of that name (id kept).
    Replace,
    /// Take it as a new skill under another label.
    Rename,
    Skip,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "SkillImportItem")]
pub struct ImportItem {
    pub path: String,
    pub action: ImportAction,
    /// The new label for `rename`.
    #[serde(default)]
    pub name: Option<String>,
}

impl ImportItem {
    /// A `rename` without a new label cannot be applied.
    pub fn unresolved(&self) -> bool {
        self.action == ImportAction::Rename
            && self.name.as_deref().map(str::trim).unwrap_or("").is_empty()
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, rename = "SkillImportedRow")]
pub struct ImportedRow {
    pub path: String,
    pub skill: Skill,
    pub action: &'static str,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, rename = "SkillImportFailedRow")]
pub struct FailedRow {
    pub path: String,
    pub error: String,
    pub code: String,
}

impl FailedRow {
    pub fn new(path: &str, error: ImportError) -> Self {
        FailedRow {
            path: path.to_string(),
            code: error.row_code().to_string(),
            error: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, rename = "SkillImportReport")]
pub struct ImportReport {
    pub folder: SkillFolder,
    pub imported: Vec<ImportedRow>,
    pub failed: Vec<FailedRow>,
    pub commit: Option<String>,
}

/// Where the imported skills land: an existing folder, or a new one.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImportFolder {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

pub fn provenance_for(source: &ParsedSource, commit: Option<&str>, path: &str) -> Provenance {
    Provenance {
        url: source.url.clone(),
        git_ref: source.git_ref.clone(),
        commit: commit.map(str::to_string),
        path: path.to_string(),
    }
}

pub fn folder_provenance_for(
    source: &ParsedSource,
    commit: Option<&str>,
    candidates: &[Candidate],
    imported_at: String,
) -> FolderProvenance {
    FolderProvenance {
        url: source.url.clone(),
        git_ref: source.git_ref.clone(),
        commit: commit.map(str::to_string),
        path: source.path.clone(),
        imported_at,
        found: candidates.len() as i64,
        invalid: candidates.iter().filter(|c| !c.valid).count() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::source::parse_source;

    fn candidate(path: &str, name: &str, valid: bool) -> Candidate {
        Candidate {
            path: path.into(),
            name: name.into(),
            description: String::new(),
            valid,
            reason: None,
            code: None,
            file_count: 0,
        }
    }

    fn skill(id: &str, provenance: Option<Provenance>) -> Skill {
        Skill {
            id: id.into(),
            name: "pdf".into(),
            description: String::new(),
            folder_id: Some("skf-1".into()),
            locked: false,
            extension: None,
            source: provenance,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn a_candidate_is_new_taken_same_commit_or_invalid() {
        let source = parse_source("https://github.com/o/r", None).unwrap();
        let pdf = candidate("skills/pdf", "pdf", true);
        assert_eq!(
            classify(pdf.clone(), None, &source, Some("c1"), &[]).status,
            CandidateStatus::New
        );
        let other = skill("a", None);
        let taken = classify(pdf.clone(), Some(&other), &source, Some("c1"), &[]);
        assert_eq!(taken.status, CandidateStatus::NameTaken);
        assert_eq!(taken.existing.unwrap().folder_id.as_deref(), Some("skf-1"));
        let same = skill("b", Some(provenance_for(&source, Some("c1"), "skills/pdf")));
        assert_eq!(
            classify(pdf.clone(), Some(&same), &source, Some("c1"), &[]).status,
            CandidateStatus::SameCommit
        );
        // Without a commit (a plain local folder) nothing is "the same commit".
        assert_eq!(
            classify(pdf, Some(&same), &source, None, &[]).status,
            CandidateStatus::NameTaken
        );
        assert_eq!(
            classify(candidate("bad", "bad", false), None, &source, None, &[]).status,
            CandidateStatus::Invalid
        );
    }

    #[test]
    fn parents_are_distinct_and_sorted() {
        let all = [
            candidate("b/x", "x", true),
            candidate("a/y", "y", true),
            candidate("a/z", "z", true),
            candidate("root", "r", true),
        ];
        assert_eq!(parents_of(&all), vec!["", "a", "b"]);
    }

    #[test]
    fn a_rename_needs_a_label() {
        let item = ImportItem {
            path: "p".into(),
            action: ImportAction::Rename,
            name: Some("  ".into()),
        };
        assert!(item.unresolved());
    }
}
