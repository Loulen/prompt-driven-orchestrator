//! « What's new » (#698): the release notes of every version the daemon missed,
//! or the changelog embedded in this build.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::release::{missed_releases, Release};

/// The `CHANGELOG.md` at the repository root, embedded at compile time: the
/// fallback body of « What's new » when the release list is unavailable, and the
/// body shown to an up-to-date daemon. It lists breaking changes only.
pub const EMBEDDED_CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// Where the changelog lives, or why it could not come from the releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ChangelogSource {
    /// Assembled from the release notes strictly newer than the installed version.
    Releases,
    /// The `CHANGELOG.md` embedded in this build.
    Embedded,
}

/// The assembled « What's new » payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "UpdateChangelog")]
pub struct Changelog {
    pub installed_version: String,
    pub latest_version: Option<String>,
    /// Versions strictly newer than the installed one, newest first. Empty when
    /// up to date or when the list was unavailable.
    pub missed_versions: Vec<String>,
    pub source: ChangelogSource,
    /// Set exactly when `source == embedded` because the releases were NOT
    /// available (check off, source unreachable, list fetch failed). `None` for
    /// an up-to-date daemon: showing the embedded changelog is then the nominal
    /// answer, not a fallback.
    pub fallback_reason: Option<String>,
    pub markdown: String,
}

/// One markdown document from the missed releases: `## vX.Y.Z`, then a muted
/// `Released <date> · [GitHub release](url)` line, then the notes verbatim.
pub fn render_releases_markdown(missed: &[Release]) -> String {
    let mut md = String::new();
    for r in missed {
        md.push_str(&format!("## v{}\n\n", r.version));
        let mut meta: Vec<String> = Vec::new();
        if let Some(at) = r.published_at.as_deref() {
            let date = at.split('T').next().unwrap_or(at);
            meta.push(format!("Released {date}"));
        }
        if let Some(url) = r.html_url.as_deref() {
            meta.push(format!("[GitHub release]({url})"));
        }
        if !meta.is_empty() {
            md.push_str(&format!("*{}*\n\n", meta.join(" · ")));
        }
        let body = r.body.trim();
        if body.is_empty() {
            md.push_str("_No release notes._\n\n");
        } else {
            md.push_str(body);
            md.push_str("\n\n");
        }
    }
    md.trim_end().to_string()
}

/// Assemble the changelog from a release list: missed versions (newest first) and
/// their markdown. With nothing newer, the embedded changelog is the body and
/// `missed_versions` is empty; `fallback_reason` stays `None` (nominal up-to-date).
pub fn assemble_changelog(
    installed: &str,
    latest: Option<&str>,
    releases: &[Release],
) -> Changelog {
    let missed = missed_releases(installed, releases);
    if missed.is_empty() {
        return Changelog {
            installed_version: installed.to_string(),
            latest_version: latest.map(str::to_string),
            missed_versions: Vec::new(),
            source: ChangelogSource::Embedded,
            fallback_reason: None,
            markdown: EMBEDDED_CHANGELOG.to_string(),
        };
    }
    Changelog {
        installed_version: installed.to_string(),
        latest_version: latest.map(str::to_string),
        missed_versions: missed.iter().map(|r| r.version.clone()).collect(),
        source: ChangelogSource::Releases,
        fallback_reason: None,
        markdown: render_releases_markdown(&missed),
    }
}

/// The embedded changelog as an explicit fallback, with the reason it is one.
pub fn embedded_changelog(installed: &str, latest: Option<&str>, reason: &str) -> Changelog {
    Changelog {
        installed_version: installed.to_string(),
        latest_version: latest.map(str::to_string),
        missed_versions: Vec::new(),
        source: ChangelogSource::Embedded,
        fallback_reason: Some(reason.to_string()),
        markdown: EMBEDDED_CHANGELOG.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(v: &str, body: &str) -> Release {
        Release {
            version: v.to_string(),
            published_at: Some(format!("2026-09-0{}T10:00:00Z", v.len() % 9 + 1)),
            html_url: Some(format!(
                "https://github.com/Loulen/PDO-v2/releases/tag/v{v}"
            )),
            body: body.to_string(),
        }
    }

    #[test]
    fn assembled_markdown_has_one_heading_per_version_newest_first() {
        let all = vec![
            rel("1.59.0", "- first\n"),
            rel("1.60.0", ""),
            rel("1.58.1", "old"),
        ];
        let c = assemble_changelog("1.58.1", Some("1.60.0"), &all);
        assert_eq!(c.source, ChangelogSource::Releases);
        assert_eq!(c.fallback_reason, None);
        assert_eq!(c.missed_versions, ["1.60.0", "1.59.0"]);
        let h60 = c.markdown.find("## v1.60.0").unwrap();
        let h59 = c.markdown.find("## v1.59.0").unwrap();
        assert!(h60 < h59, "newest first:\n{}", c.markdown);
        assert!(!c.markdown.contains("## v1.58.1"));
        assert!(c.markdown.contains("*Released 2026-09-"));
        assert!(c.markdown.contains("[GitHub release](https://github.com/"));
        assert!(c.markdown.contains("- first"));
        assert!(c.markdown.contains("_No release notes._"));
    }

    #[test]
    fn up_to_date_and_fallback_use_the_embedded_changelog() {
        let all = vec![rel("1.58.1", "same")];
        let c = assemble_changelog("1.58.1", Some("1.58.1"), &all);
        assert_eq!(c.source, ChangelogSource::Embedded);
        assert_eq!(
            c.fallback_reason, None,
            "up to date is nominal, not a fallback"
        );
        assert!(c.missed_versions.is_empty());
        assert_eq!(c.markdown, EMBEDDED_CHANGELOG);
        assert!(EMBEDDED_CHANGELOG.starts_with("# Changelog"));

        let f = embedded_changelog("1.58.1", None, "The update check is off.");
        assert_eq!(f.source, ChangelogSource::Embedded);
        assert_eq!(
            f.fallback_reason.as_deref(),
            Some("The update check is off.")
        );
        assert_eq!(f.latest_version, None);
        assert_eq!(f.markdown, EMBEDDED_CHANGELOG);
    }
}
