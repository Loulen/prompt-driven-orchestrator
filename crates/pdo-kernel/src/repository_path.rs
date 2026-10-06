//! How a repository path is matched against a Projet's members, shared by the
//! contexts that place an Operation in a Projet (the Run, the workspace).
//!
//! A Projet's members are compared verbatim (ADR-0033), but a Run works in
//! whatever directory its path names: `/repo/`, `/repo/.` or a link to `/repo`
//! must not take a Run out of the Projet owning `/repo`, nor out of a read
//! refused in it (CORE-5).

use std::path::{Path, PathBuf};

/// The ways `path` may be spelled as a Projet's member, in order: verbatim,
/// lexically normalized (no trailing `/`, no `.`), then canonical on disk.
pub fn member_spellings(path: &str) -> Vec<String> {
    let mut spellings = vec![path.to_string()];
    let lexical = Path::new(path).components().collect::<PathBuf>();
    let canonical = std::fs::canonicalize(path).ok();
    for other in [Some(lexical), canonical].into_iter().flatten() {
        let other = other.to_string_lossy().into_owned();
        if !spellings.contains(&other) {
            spellings.push(other);
        }
    }
    spellings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_tried_verbatim_then_normalized_then_resolved() {
        assert_eq!(
            member_spellings("/no/such/repo/./"),
            ["/no/such/repo/./", "/no/such/repo"]
        );
        assert_eq!(member_spellings("/no/such/repo"), ["/no/such/repo"]);
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = std::fs::canonicalize(&real).unwrap();
        assert_eq!(
            member_spellings(&link.to_string_lossy()).last().unwrap(),
            &canonical.to_string_lossy().into_owned()
        );
    }
}
