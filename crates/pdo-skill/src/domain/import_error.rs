//! Why a scan, an import or an update from a Source did not happen.

use std::time::Duration;

use super::error::SkillError;

/// A shallow clone that takes longer than this is refused, not awaited forever.
pub const CLONE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    EmptySource,
    /// The input is neither a repository URL nor an absolute local folder.
    InvalidSource(String),
    LocalNotFound(String),
    /// `git clone` failed; carries git's stderr for the callout.
    CloneFailed(String),
    CloneTimeout,
    Cancelled,
    /// The `scan_id` is unknown or its clone was cleaned up; re-scan.
    ScanExpired,
    /// The folder addressed for a rescan / update has no provenance.
    NotASourceFolder,
    /// A candidate path named in an import is not in the scan.
    UnknownCandidate(String),
    /// A collision row was sent without a resolution.
    UnresolvedCollision(String),
    Skill(SkillError),
    Io(String),
}

impl ImportError {
    /// Stable machine-readable code next to the message.
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptySource => "empty_source",
            Self::InvalidSource(_) => "invalid_source",
            Self::LocalNotFound(_) => "local_not_found",
            Self::CloneFailed(_) => "clone_failed",
            Self::CloneTimeout => "clone_timeout",
            Self::Cancelled => "cancelled",
            Self::ScanExpired => "scan_expired",
            Self::NotASourceFolder => "not_a_source_folder",
            Self::UnknownCandidate(_) => "unknown_candidate",
            Self::UnresolvedCollision(_) => "unresolved_collision",
            Self::Skill(_) => "skill",
            Self::Io(_) => "io",
        }
    }

    /// The code a failed import row reports: the gate check that refused it,
    /// `duplicate_name`, or the import error's own code.
    pub fn row_code(&self) -> &'static str {
        match self {
            Self::Skill(SkillError::DuplicateName { .. }) => "duplicate_name",
            Self::Skill(inner) => inner.validation_code(),
            other => other.code(),
        }
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySource => write!(f, "no source given"),
            Self::InvalidSource(input) => write!(
                f,
                "`{input}` is neither a repository URL (https://…, git@…, file://…) nor an absolute local folder"
            ),
            Self::LocalNotFound(path) => write!(f, "`{path}` is not a folder on this machine"),
            Self::CloneFailed(stderr) => write!(f, "{}", stderr.trim()),
            Self::CloneTimeout => write!(f, "the clone did not finish within {}s", CLONE_TIMEOUT.as_secs()),
            Self::Cancelled => write!(f, "scan cancelled"),
            Self::ScanExpired => write!(f, "this scan expired; scan the source again"),
            Self::NotASourceFolder => write!(f, "this folder was not imported from a source"),
            Self::UnknownCandidate(path) => write!(f, "`{path}` is not part of the scan"),
            Self::UnresolvedCollision(name) => {
                write!(f, "`{name}` is already taken: choose replace, rename or skip")
            }
            Self::Skill(error) => write!(f, "{error}"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<SkillError> for ImportError {
    fn from(error: SkillError) -> Self {
        Self::Skill(error)
    }
}

impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_row_names_the_check_that_refused_it() {
        let dup = ImportError::Skill(SkillError::DuplicateName {
            existing_id: "a".into(),
            name: "tdd".into(),
        });
        assert_eq!(dup.row_code(), "duplicate_name");
        assert_eq!(
            ImportError::Skill(SkillError::EmptyBody).row_code(),
            "empty_body"
        );
        assert_eq!(
            ImportError::Skill(SkillError::FolderNotFound).row_code(),
            "invalid"
        );
        assert_eq!(
            ImportError::UnknownCandidate("x".into()).row_code(),
            "unknown_candidate"
        );
    }
}
