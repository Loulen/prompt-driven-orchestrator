//! Create a git repository from scratch, files and first commit included (#824).
//!
//! A **generic** verb: parent folder, name, initial files in, absolute path out. It
//! knows nothing about tours (ADR-0071 §3) — the *First run* tour is merely its
//! first caller, and asks for `/tmp` + `pdo-tutorial`. Anything else that needs a
//! throwaway repository can call it with its own three arguments.
//!
//! Three outcomes, and they are the whole contract:
//!
//! 1. Nothing at the target path, or an empty folder → create it, `git init` on
//!    `main`, write the files, one commit (even without files).
//!    [`ScaffoldOutcome::created`] is `true`.
//! 2. A **git repository** already there → hand back its path, touch nothing, do
//!    NOT commit again. `created` is `false`. This is what makes a second run of
//!    the tour free, and what lets the user keep whatever they did in there.
//! 3. A non-empty directory that is **not** a repository → a named refusal.
//!    Never "fix" it by initialising a repo over someone's folder: the path was
//!    not ours, and the caller is entitled to say so to the user verbatim.
//!
//! The commit is made under an identity written into the repository's own config
//! rather than taken from the machine's. A daemon that borrowed `~/.gitconfig`
//! would fail on a host that has none (`git commit` exits 128 asking who you are),
//! and would sign a throwaway repo with the user's real name for no reason.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One file to write before the first commit. `path` is relative to the new repo
/// and is rejected if it tries to leave it.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, optional_fields = nullable, rename = "CreateRepoFile")]
pub struct ScaffoldFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldOutcome {
    pub path: PathBuf,
    /// `false` when an existing repository was reused as is.
    pub created: bool,
}

/// The identity of the initial commit, local to the created repository.
pub const AUTHOR_NAME: &str = "PDO";
pub const AUTHOR_EMAIL: &str = "pdo@localhost";

/// The branch every repository this verb creates starts on. Stated rather than
/// inherited: `init.defaultBranch` varies per machine, and a caller that then
/// offers "source branch" would show whatever the host happened to prefer.
pub const INITIAL_BRANCH: &str = "main";

/// The folder name of the new repository: trimmed, a single path segment.
pub fn check_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name must not be empty".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err(format!("name must be a single folder name: {name}"));
    }
    Ok(trimmed)
}

/// Reject anything that would write outside the new repository — an absolute
/// path, a `..` segment, or an empty name. The caller supplies these, and a
/// caller is not a threat model, but a silently mis-scoped write is a bug that
/// only shows up as a corrupted file somewhere else.
pub fn check_relative(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("file path must not be empty".into());
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return Err(format!(
            "file path must be relative to the repository: {path}"
        ));
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "file path must not climb out of the repository: {path}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_one_trimmed_segment() {
        assert_eq!(check_name("  tuto "), Ok("tuto"));
        assert!(check_name("").is_err());
        assert!(check_name("a/b").is_err());
        assert!(check_name("..").is_err());
        assert!(check_name("a\\b").is_err());
    }

    #[test]
    fn a_file_path_stays_inside_the_repository() {
        assert_eq!(check_relative("src/main.rs"), Ok(()));
        assert!(check_relative("  ").is_err());
        assert!(check_relative("/etc/passwd").is_err());
        assert!(check_relative("../outside").is_err());
        assert!(check_relative("a/../../b").is_err());
    }
}
