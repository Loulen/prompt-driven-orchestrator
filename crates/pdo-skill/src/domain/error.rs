//! Why a write to the bank was refused. The HTTP adapter maps each variant to a
//! status; the message is the reason the popup shows in place.

use std::fmt;

use pdo_kernel::authorization::Denied;

/// The per-file ceiling of an upload (#671 design: 10 MB). A reference file is
/// a cheatsheet or a fixture the agent reads, not a dataset.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    /// No `---` block at the top of the text.
    NoFrontmatter,
    MalformedFrontmatter(String),
    MissingName,
    /// `name` must be kebab-case: `^[a-z0-9]+(-[a-z0-9]+)*$`.
    NameNotKebabCase(String),
    MissingDescription,
    EmptyBody,
    /// Another skill already carries this label, case-insensitively.
    DuplicateName {
        existing_id: String,
        name: String,
    },
    EmptyLabel,
    NotFound,
    FolderNotFound,
    EmptyFolderName,
    /// Moving a folder under itself or one of its descendants.
    FolderCycle,
    /// A reference-file path that would leave the skill's folder, is absolute,
    /// empty, or otherwise not a plain relative `a/b/c.ext` (#671 AC: 400).
    InvalidPath(String),
    /// `SKILL.md` is the skill's text, not a reference file: it is neither
    /// uploaded nor deleted through the files endpoints (`PUT` it instead).
    SkillMdReserved,
    FileNotFound(String),
    /// One file above [`MAX_FILE_BYTES`].
    FileTooLarge {
        path: String,
        size: u64,
    },
    /// `from_path` (the explorer pick) is not a readable regular file.
    SourceNotAFile(String),
    /// The seeded skill (`pdo-orchestrate`, #722): no edit, no delete, on any
    /// bank surface — it is recreated at every daemon start.
    Locked {
        id: String,
    },
    /// An extension refused the Operation.
    Denied(Denied),
    Storage(String),
}

impl SkillError {
    /// A stable machine-readable code next to the human message, so the popup
    /// can light the right check without parsing English.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoFrontmatter => "no_frontmatter",
            Self::MalformedFrontmatter(_) => "malformed_frontmatter",
            Self::MissingName => "missing_name",
            Self::NameNotKebabCase(_) => "name_not_kebab_case",
            Self::MissingDescription => "missing_description",
            Self::EmptyBody => "empty_body",
            Self::DuplicateName { .. } => "duplicate_name",
            Self::EmptyLabel => "empty_label",
            Self::NotFound => "not_found",
            Self::FolderNotFound => "folder_not_found",
            Self::EmptyFolderName => "empty_folder_name",
            Self::FolderCycle => "folder_cycle",
            Self::InvalidPath(_) => "invalid_path",
            Self::SkillMdReserved => "skill_md_reserved",
            Self::FileNotFound(_) => "file_not_found",
            Self::FileTooLarge { .. } => "file_too_large",
            Self::SourceNotAFile(_) => "source_not_a_file",
            Self::Locked { .. } => "locked",
            Self::Denied(_) => "denied",
            Self::Storage(_) => "storage",
        }
    }

    /// The code of a `SKILL.md` that does not pass the gate, as a scan reports
    /// it on its candidate; `invalid` for anything that is not a gate check.
    pub fn validation_code(&self) -> &'static str {
        match self {
            Self::NoFrontmatter
            | Self::MalformedFrontmatter(_)
            | Self::MissingName
            | Self::NameNotKebabCase(_)
            | Self::MissingDescription
            | Self::EmptyBody => self.code(),
            _ => "invalid",
        }
    }
}

pub(crate) fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrontmatter => write!(
                f,
                "no frontmatter block: a SKILL.md starts with `---`, then `name:` and \
                 `description:`, then `---`"
            ),
            Self::MalformedFrontmatter(reason) => {
                write!(f, "the frontmatter is not valid YAML: {reason}")
            }
            Self::MissingName => write!(f, "the frontmatter has no `name`"),
            Self::NameNotKebabCase(name) => write!(
                f,
                "`name: {name}` is not kebab-case (lowercase letters, digits and single \
                 hyphens, e.g. `code-review`)"
            ),
            Self::MissingDescription => write!(
                f,
                "the frontmatter has no `description`; the harness would ignore this skill"
            ),
            Self::EmptyBody => write!(f, "the body after the frontmatter is empty"),
            Self::DuplicateName { name, .. } => write!(
                f,
                "a skill named `{name}` already exists (names are unique case-insensitively)"
            ),
            Self::EmptyLabel => write!(f, "a skill name cannot be blank"),
            Self::NotFound => write!(f, "no such skill"),
            Self::FolderNotFound => write!(f, "no such skill folder"),
            Self::EmptyFolderName => write!(f, "a folder name cannot be blank"),
            Self::FolderCycle => write!(f, "a folder cannot be moved under itself"),
            Self::InvalidPath(path) => write!(
                f,
                "`{path}` is not a valid file path inside the skill folder (relative, no `..`, \
                 no leading `/`)"
            ),
            Self::SkillMdReserved => write!(
                f,
                "SKILL.md is the skill's text, not a reference file: edit it, do not upload or \
                 delete it"
            ),
            Self::FileNotFound(path) => write!(f, "no file `{path}` in this skill"),
            Self::FileTooLarge { path, size } => write!(
                f,
                "`{path}` is {} — larger than the {} MB limit",
                human_size(*size),
                MAX_FILE_BYTES / (1024 * 1024)
            ),
            Self::SourceNotAFile(path) => {
                write!(
                    f,
                    "`{path}` is not a readable file (drop files, not folders)"
                )
            }
            Self::Locked { id } => write!(
                f,
                "`{id}` is seeded and managed by PDO (its own skill, or one an extension \
                 provides): it is recreated at every daemon start and locked against editing \
                 and deletion"
            ),
            Self::Denied(denied) => write!(f, "{denied}"),
            Self::Storage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SkillError {}

impl From<Denied> for SkillError {
    fn from(denied: Denied) -> Self {
        Self::Denied(denied)
    }
}

impl From<std::io::Error> for SkillError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_the_popup_says_them() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(2048), "2.0 KB");
        let big = SkillError::FileTooLarge {
            path: "big.bin".into(),
            size: MAX_FILE_BYTES + 1,
        };
        assert_eq!(
            big.to_string(),
            "`big.bin` is 10.0 MB — larger than the 10 MB limit"
        );
    }

    #[test]
    fn only_the_gate_checks_keep_their_code_on_a_candidate() {
        assert_eq!(
            SkillError::MissingDescription.validation_code(),
            "missing_description"
        );
        assert_eq!(SkillError::NotFound.validation_code(), "invalid");
        assert_eq!(SkillError::NotFound.code(), "not_found");
    }
}
