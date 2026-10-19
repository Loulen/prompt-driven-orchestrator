//! How a repository is named in the telemetry (ADR-0090 §3): by its remote URL,
//! otherwise by the name of its folder — never by a local path. A remote that
//! is itself a path (`/srv/git/app.git`, `file://…`, `../app`) is a path too,
//! so it falls back to the folder name as well.

use std::path::Path;

/// The label of the repository at `folder`, whose remote is `remote` (as
/// `git remote get-url` prints it).
pub fn repository_label(remote: Option<&str>, folder: &Path) -> String {
    remote
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .and_then(public_remote)
        .unwrap_or_else(|| folder_name(folder))
}

/// The remote without its credentials, or `None` when it is a local path.
fn public_remote(remote: &str) -> Option<String> {
    if let Some((scheme, rest)) = remote.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return None;
        }
        // `https://user:token@host/org/app.git` → `https://host/org/app.git`:
        // a token in the URL must never leave with the metrics.
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let path = if path.is_empty() {
            String::new()
        } else {
            format!("/{path}")
        };
        return Some(format!("{scheme}://{host}{path}"));
    }
    // The scp-like form `git@host:org/app.git` names a host before the `:`; a
    // path has a `/` (or nothing) there instead, and a drive letter one char.
    match remote.split_once(':') {
        Some((host, _)) if host.len() > 1 && !host.contains('/') && !host.contains('\\') => {
            Some(remote.to_string())
        }
        _ => None,
    }
}

/// The last component of `folder`, never the path that leads to it.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> &'static Path {
        Path::new("/home/someone/work/app")
    }

    #[test]
    fn an_https_remote_names_the_repository() {
        assert_eq!(
            repository_label(Some("https://git.example.org/org/app.git"), folder()),
            "https://git.example.org/org/app.git"
        );
    }

    #[test]
    fn credentials_in_the_remote_are_dropped() {
        assert_eq!(
            repository_label(
                Some("https://user:s3cret@git.example.org/org/app.git"),
                folder()
            ),
            "https://git.example.org/org/app.git"
        );
    }

    #[test]
    fn an_scp_like_remote_names_the_repository() {
        assert_eq!(
            repository_label(Some("git@git.example.org:org/app.git"), folder()),
            "git@git.example.org:org/app.git"
        );
    }

    #[test]
    fn no_remote_names_the_folder_never_the_path() {
        assert_eq!(repository_label(None, folder()), "app");
        assert_eq!(repository_label(Some("  "), folder()), "app");
    }

    #[test]
    fn a_remote_that_is_a_path_names_the_folder() {
        for remote in [
            "/srv/git/app.git",
            "../app",
            "file:///srv/git/app.git",
            "C:\\repos\\app",
        ] {
            assert_eq!(repository_label(Some(remote), folder()), "app", "{remote}");
        }
    }
}
