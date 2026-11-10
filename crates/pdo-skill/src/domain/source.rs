//! Reading a **Source** (#670, CONTEXT.md §*Source*): a git repository URL
//! (root, branch, or `/tree/<branch>/<path>` sub-folder), an SSH URL, a
//! `file://` URL, or a local folder. The daemon's reading is the one that
//! counts; the popup mirrors it for its live chips.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::import_error::ImportError;
use super::skill::SkillFolder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SkillSourceKind")]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Git,
    Local,
}

/// What the daemon understood of the typed source. Mirrored client-side for the
/// live chips; the daemon's reading is the one that counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "ParsedSkillSource")]
pub struct ParsedSource {
    pub kind: SourceKind,
    /// The clone URL (git) or the absolute folder (local). This is the `url`
    /// stored in provenance.
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Sub-folder to scan, `/`-separated, `""` for the root.
    pub path: String,
    /// `owner/repo` for a forge URL, the folder name otherwise.
    pub repo: String,
    /// Default name of the Source folder: `repo`, suffixed ` · <last path segment>`.
    pub suggested_folder: String,
}

fn strip_git_suffix(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

fn last_segment(path: &str) -> Option<&str> {
    path.trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}

pub fn suggested_folder(repo: &str, path: &str) -> String {
    match last_segment(path) {
        Some(last) => format!("{repo} · {last}"),
        None => repo.to_string(),
    }
}

/// Parse the typed source. `home` expands a leading `~`.
pub fn parse_source(input: &str, home: Option<&Path>) -> Result<ParsedSource, ImportError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ImportError::EmptySource);
    }
    // SSH: git@host:owner/repo.git
    if input.starts_with("git@") || input.starts_with("ssh://") {
        let after_host = if let Some(rest) = input.strip_prefix("ssh://") {
            rest.split_once('/').map_or("", |x| x.1)
        } else {
            input.split_once(':').map_or("", |x| x.1)
        };
        let repo = strip_git_suffix(after_host.trim_matches('/')).to_string();
        if repo.is_empty() {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url: input.to_string(),
            git_ref: None,
            path: String::new(),
            repo: repo.clone(),
            suggested_folder: repo,
        });
    }
    if let Some(rest) = input.strip_prefix("file://") {
        let repo = last_segment(strip_git_suffix(rest))
            .unwrap_or("repo")
            .to_string();
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url: input.to_string(),
            git_ref: None,
            path: String::new(),
            repo: repo.clone(),
            suggested_folder: repo,
        });
    }
    if let Some((scheme, rest)) = input.split_once("://") {
        if scheme != "http" && scheme != "https" {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let mut parts = rest.split('/').filter(|s| !s.is_empty());
        let host = parts
            .next()
            .ok_or_else(|| ImportError::InvalidSource(input.to_string()))?;
        let segments: Vec<&str> = parts.collect();
        if segments.len() < 2 {
            return Err(ImportError::InvalidSource(input.to_string()));
        }
        let owner = segments[0];
        let repo_name = strip_git_suffix(segments[1]);
        let repo = format!("{owner}/{repo_name}");
        // GitHub: /tree/<ref>/<path>, /blob/<ref>/<file>; GitLab: /-/tree/<ref>/<path>.
        let mut tail = &segments[2..];
        if tail.first() == Some(&"-") {
            tail = &tail[1..];
        }
        let (git_ref, path) = match tail.first() {
            Some(&"tree") | Some(&"blob") if tail.len() >= 2 => {
                let is_blob = tail[0] == "blob";
                let mut path_segments: Vec<&str> = tail[2..].to_vec();
                if is_blob {
                    path_segments.pop();
                }
                (Some(tail[1].to_string()), path_segments.join("/"))
            }
            _ => (None, String::new()),
        };
        let url = format!("{scheme}://{host}/{owner}/{repo_name}");
        return Ok(ParsedSource {
            kind: SourceKind::Git,
            url,
            git_ref,
            path: path.clone(),
            repo: repo.clone(),
            suggested_folder: suggested_folder(&repo, &path),
        });
    }
    // Local folder.
    let expanded = if let Some(rest) = input.strip_prefix("~") {
        match home {
            Some(home) => home
                .join(rest.trim_start_matches('/'))
                .display()
                .to_string(),
            None => return Err(ImportError::InvalidSource(input.to_string())),
        }
    } else {
        input.to_string()
    };
    if !expanded.starts_with('/') {
        return Err(ImportError::InvalidSource(input.to_string()));
    }
    let normalised = expanded.trim_end_matches('/').to_string();
    let repo = last_segment(&normalised).unwrap_or("folder").to_string();
    Ok(ParsedSource {
        kind: SourceKind::Local,
        url: if normalised.is_empty() {
            "/".to_string()
        } else {
            normalised
        },
        git_ref: None,
        path: String::new(),
        repo: repo.clone(),
        suggested_folder: repo,
    })
}

/// The source a Source folder was imported from, as its provenance recorded it.
pub fn source_of_folder(folder: &SkillFolder) -> Result<ParsedSource, ImportError> {
    let provenance = folder
        .source
        .as_ref()
        .ok_or(ImportError::NotASourceFolder)?;
    let mut parsed = parse_source(&provenance.url, None)?;
    parsed.git_ref = provenance.git_ref.clone();
    parsed.path = provenance.path.clone();
    parsed.suggested_folder = suggested_folder(&parsed.repo, &parsed.path);
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_github_root_url() {
        let parsed = parse_source("https://github.com/anthropics/skills", None).unwrap();
        assert_eq!(parsed.kind, SourceKind::Git);
        assert_eq!(parsed.url, "https://github.com/anthropics/skills");
        assert_eq!(parsed.git_ref, None);
        assert_eq!(parsed.path, "");
        assert_eq!(parsed.repo, "anthropics/skills");
        assert_eq!(parsed.suggested_folder, "anthropics/skills");
    }

    #[test]
    fn parses_a_github_tree_url_with_branch_and_sub_folder() {
        let parsed = parse_source(
            "https://github.com/anthropics/skills/tree/main/skills/engineering",
            None,
        )
        .unwrap();
        assert_eq!(parsed.url, "https://github.com/anthropics/skills");
        assert_eq!(parsed.git_ref.as_deref(), Some("main"));
        assert_eq!(parsed.path, "skills/engineering");
        assert_eq!(parsed.suggested_folder, "anthropics/skills · engineering");
    }

    #[test]
    fn parses_a_branch_only_tree_url_and_a_dot_git_suffix() {
        let parsed = parse_source("https://github.com/o/r.git/tree/dev", None).unwrap();
        assert_eq!(parsed.url, "https://github.com/o/r");
        assert_eq!(parsed.git_ref.as_deref(), Some("dev"));
        assert_eq!(parsed.path, "");
        let parsed = parse_source("https://gitlab.com/o/r/-/tree/main/skills", None).unwrap();
        assert_eq!(parsed.git_ref.as_deref(), Some("main"));
        assert_eq!(parsed.path, "skills");
    }

    #[test]
    fn parses_ssh_file_and_local_sources() {
        let ssh = parse_source("git@github.com:acme/private-skills.git", None).unwrap();
        assert_eq!(ssh.kind, SourceKind::Git);
        assert_eq!(ssh.url, "git@github.com:acme/private-skills.git");
        assert_eq!(ssh.repo, "acme/private-skills");

        let file = parse_source("file:///tmp/fixture.git", None).unwrap();
        assert_eq!(file.kind, SourceKind::Git);
        assert_eq!(file.repo, "fixture");

        let local = parse_source("~/Documents/skills-repo/", Some(Path::new("/home/me"))).unwrap();
        assert_eq!(local.kind, SourceKind::Local);
        assert_eq!(local.url, "/home/me/Documents/skills-repo");
        assert_eq!(local.suggested_folder, "skills-repo");
    }

    #[test]
    fn refuses_garbage() {
        assert_eq!(
            parse_source("   ", None).unwrap_err(),
            ImportError::EmptySource
        );
        assert!(matches!(
            parse_source("not a source", None).unwrap_err(),
            ImportError::InvalidSource(_)
        ));
        assert!(matches!(
            parse_source("https://github.com/only-owner", None).unwrap_err(),
            ImportError::InvalidSource(_)
        ));
    }
}
