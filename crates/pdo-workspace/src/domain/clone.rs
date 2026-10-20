//! A repository given by its URL: whether a text is such a URL, the folder
//! its clone goes in, the clones folder setting, whether a repository already
//! is that clone, and what `git clone` says along the way.

use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use super::remote::shown_remote;

/// The clones folder when none is stored: under the daemon's home.
pub const DEFAULT_CLONES_FOLDER: &str = "~/pdo/repos";

/// Whether `text` names a remote repository rather than a path on disk: a
/// `scheme://` URL git can clone (`https`, `http`, `ssh`, `git`, `file`,
/// `git+ssh`), or
/// an scp-like `user@host:path`.
pub fn is_remote_url(text: &str) -> bool {
    let text = text.trim();
    if let Some((scheme, rest)) = text.split_once("://") {
        let known = ["https", "http", "ssh", "git", "file", "git+ssh", "ssh+git"]
            .iter()
            .any(|s| scheme.eq_ignore_ascii_case(s));
        return known && !rest.trim_matches('/').is_empty();
    }
    // scp-like: `user@host:path`, no slash before the colon.
    match text.split_once(':') {
        Some((authority, path)) => {
            authority.contains('@')
                && !authority.contains('/')
                && !authority.starts_with('@')
                && !authority.ends_with('@')
                && !path.trim_matches('/').is_empty()
                && !text.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// The name of the repository `url` points at: its last path segment, without
/// `.git`. `None` when the URL ends on nothing a folder could be named after.
pub fn repository_name(url: &str) -> Option<String> {
    let url = url.trim();
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, p)| p)?,
        None => url.split_once(':').map(|(_, p)| p)?,
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() || name == "." || name == ".." || name.starts_with('-') {
        return None;
    }
    Some(name.to_string())
}

/// The clones folder as it is stored: trimmed, without a trailing `/`, `None`
/// for a blank one (back to the default). An absolute path or one under `~`;
/// anything else is refused, naming why.
pub fn normalize_clones_folder(raw: &str) -> Result<Option<String>, String> {
    let folder = raw.trim();
    if folder.is_empty() {
        return Ok(None);
    }
    let trimmed = folder.trim_end_matches('/');
    let folder = if trimmed.is_empty() { "/" } else { trimmed };
    if folder == "~" || folder.starts_with("~/") || folder.starts_with('/') {
        Ok(Some(folder.to_string()))
    } else {
        Err(format!(
            "the clones folder must be an absolute path or start with ~/: {folder}"
        ))
    }
}

/// `folder` on disk: `~` stands for `home`. `None` when it needs a home and
/// there is none.
pub fn expand_home(folder: &str, home: Option<&Path>) -> Option<PathBuf> {
    let folder = folder.trim();
    if folder == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = folder.strip_prefix("~/") {
        return home.map(|h| h.join(rest));
    }
    Some(PathBuf::from(folder))
}

/// Whether two remotes name the same repository: compared without the
/// credentials, the case of the scheme and the host aside, a trailing `/` or
/// `.git` aside. The path keeps its case.
pub fn same_remote(a: &str, b: &str) -> bool {
    fn key(url: &str) -> String {
        let shown = shown_remote(url);
        let trimmed = shown.trim_end_matches('/');
        let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
        // `scheme://host/path`, or the scp-like `user@host:path`.
        let (head, path) = match trimmed.split_once("://") {
            Some((scheme, rest)) => {
                let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
                (format!("{scheme}://{host}"), format!("/{path}"))
            }
            None => match trimmed.split_once(':') {
                Some((host, path)) => (host.to_string(), format!(":{path}")),
                None => (String::new(), trimmed.to_string()),
            },
        };
        format!("{}{path}", head.to_ascii_lowercase())
    }
    key(a) == key(b)
}

/// Why a full clone did not produce a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneFailure {
    /// The clone was cancelled.
    Cancelled,
    /// The clone failed; carries git's stderr (or why git could not run).
    Failed(String),
}

/// Where `git clone` is, as the page shows it: the phase git names, how far it
/// is, and the size received when git says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CloneProgress {
    /// `Receiving objects`, `Resolving deltas`…
    pub phase: String,
    pub percent: Option<u8>,
    /// `38.20 MiB` while objects arrive; `null` otherwise.
    pub size: Option<String>,
}

/// One line of `git clone --progress`, read: `Receiving objects:  64%
/// (640/1000), 38.20 MiB | 1.20 MiB/s`. `None` for a line that is no progress.
pub fn parse_progress(line: &str) -> Option<CloneProgress> {
    let line = line.trim();
    let line = line.strip_prefix("remote:").map_or(line, str::trim);
    let (phase, rest) = line.split_once(':')?;
    let phase = phase.trim();
    if phase.is_empty() || phase.contains(char::is_numeric) {
        return None;
    }
    let rest = rest.trim();
    let percent = rest
        .split_once('%')
        .and_then(|(n, _)| n.trim().parse::<u8>().ok())
        .filter(|p| *p <= 100);
    percent?;
    let size = rest
        .split_once("),")
        .map(|(_, tail)| tail.split('|').next().unwrap_or("").trim())
        .map(|s| {
            s.trim_end_matches("done.")
                .trim()
                .trim_end_matches(',')
                .trim()
        })
        .filter(|s| s.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string);
    Some(CloneProgress {
        phase: phase.to_string(),
        percent,
        size,
    })
}

/// Why a clone failed, as a sentence a user can act on: the refused
/// credentials and the URL that leads nowhere are named, git's own words
/// follow.
pub fn clone_failure(url: &str, stderr: &str) -> String {
    let shown = shown_remote(url);
    let detail = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Cloning into"))
        .collect::<Vec<_>>()
        .join(" ")
        // git may quote the URL it was given, credentials included.
        .replace(url.trim(), &shown);
    let lower = detail.to_ascii_lowercase();
    let reason = if [
        "authentication failed",
        "could not read username",
        "could not read password",
        "permission denied",
        "terminal prompts disabled",
        "invalid username or password",
        "host key verification failed",
    ]
    .iter()
    .any(|m| lower.contains(m))
    {
        format!("The daemon's git credentials were refused for {shown}")
    } else if [
        "not found",
        "does not appear to be a git repository",
        "does not exist",
        "could not resolve host",
        "unable to access",
        "no such file",
    ]
    .iter()
    .any(|m| lower.contains(m))
    {
        format!("No repository answers at {shown}")
    } else {
        format!("The clone of {shown} failed")
    };
    if detail.is_empty() {
        format!("{reason}.")
    } else {
        format!("{reason}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_told_from_paths() {
        for url in [
            "https://github.com/acme/app.git",
            "http://git.local/app",
            "ssh://git@host:22/acme/app.git",
            "git://host/app.git",
            "file:///srv/git/app.git",
            "git@github.com:acme/app.git",
            "git+ssh://git@host/app.git",
            "me@host:app",
        ] {
            assert!(is_remote_url(url), "{url}");
        }
        for path in [
            "/home/me/app",
            "~/code/app",
            "app",
            "C:/repo",
            "https://",
            "ftp://host/app",
            "git+https://host/app",
            "git@host:",
            "",
            "@host:app",
        ] {
            assert!(!is_remote_url(path), "{path}");
        }
    }

    #[test]
    fn the_name_is_the_last_segment_without_dot_git() {
        assert_eq!(
            repository_name("https://github.com/acme/analytics.git").as_deref(),
            Some("analytics")
        );
        assert_eq!(
            repository_name("git@github.com:acme/shop-web.git").as_deref(),
            Some("shop-web")
        );
        assert_eq!(
            repository_name("file:///srv/git/origin/").as_deref(),
            Some("origin")
        );
        assert_eq!(repository_name("me@host:app").as_deref(), Some("app"));
        assert_eq!(repository_name("https://host"), None);
        assert_eq!(repository_name("https://host/.git"), None);
        assert_eq!(repository_name("https://host/-x"), None);
    }

    #[test]
    fn the_clones_folder_is_stored_trimmed_cleared_or_refused() {
        assert_eq!(
            normalize_clones_folder(" ~/pdo/repos/ ")
                .unwrap()
                .as_deref(),
            Some("~/pdo/repos")
        );
        assert_eq!(
            normalize_clones_folder("/srv/clones").unwrap().as_deref(),
            Some("/srv/clones")
        );
        assert_eq!(normalize_clones_folder("  ").unwrap(), None);
        assert!(normalize_clones_folder("clones")
            .unwrap_err()
            .contains("absolute"));
    }

    #[test]
    fn the_tilde_stands_for_the_home() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand_home("~/pdo/repos", Some(home)),
            Some(PathBuf::from("/home/me/pdo/repos"))
        );
        assert_eq!(
            expand_home("~", Some(home)),
            Some(PathBuf::from("/home/me"))
        );
        assert_eq!(expand_home("~/x", None), None);
        assert_eq!(expand_home("/srv", None), Some(PathBuf::from("/srv")));
    }

    #[test]
    fn remotes_compare_without_credentials_or_dot_git() {
        assert!(same_remote(
            "https://me:tok@GitHub.com/acme/app.git",
            "https://github.com/acme/app"
        ));
        assert!(same_remote("file:///srv/app/", "file:///srv/app"));
        assert!(!same_remote(
            "https://github.com/acme/app",
            "https://github.com/acme/other"
        ));
        assert!(!same_remote("file:///srv/App", "file:///srv/app"));
        assert!(same_remote(
            "git@GitHub.com:acme/App.git",
            "git@github.com:acme/App"
        ));
    }

    #[test]
    fn progress_lines_are_read() {
        assert_eq!(
            parse_progress("Receiving objects:  64% (640/1000), 38.20 MiB | 1.20 MiB/s"),
            Some(CloneProgress {
                phase: "Receiving objects".into(),
                percent: Some(64),
                size: Some("38.20 MiB".into()),
            })
        );
        assert_eq!(
            parse_progress("Resolving deltas: 100% (12/12), done."),
            Some(CloneProgress {
                phase: "Resolving deltas".into(),
                percent: Some(100),
                size: None,
            })
        );
        assert_eq!(
            parse_progress("remote: Counting objects:  50% (5/10)"),
            Some(CloneProgress {
                phase: "Counting objects".into(),
                percent: Some(50),
                size: None,
            })
        );
        assert_eq!(parse_progress("Cloning into '/tmp/x'..."), None);
        assert_eq!(
            parse_progress("remote: Enumerating objects: 12, done."),
            None
        );
    }

    #[test]
    fn a_failure_names_the_credentials_or_the_url() {
        let auth = clone_failure(
            "https://me:tok@host/acme/app.git",
            "Cloning into 'app'...\nfatal: Authentication failed for 'https://host/acme/app.git/'",
        );
        assert!(
            auth.starts_with(
                "The daemon's git credentials were refused for https://host/acme/app.git"
            ),
            "{auth}"
        );
        assert!(!auth.contains("tok"), "{auth}");
        let quoted = clone_failure(
            "https://me:tok@host/acme/app.git",
            "fatal: unable to access 'https://me:tok@host/acme/app.git/': Could not resolve host",
        );
        assert!(!quoted.contains("tok"), "{quoted}");
        let missing = clone_failure(
            "file:///nowhere",
            "fatal: '/nowhere' does not appear to be a git repository",
        );
        assert!(
            missing.starts_with("No repository answers at file:///nowhere"),
            "{missing}"
        );
        assert_eq!(
            clone_failure("https://h/a", ""),
            "The clone of https://h/a failed."
        );
    }
}
