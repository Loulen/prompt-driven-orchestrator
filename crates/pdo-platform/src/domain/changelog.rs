//! « What's new » (#698): the release notes of every version the daemon missed,
//! or the changelog embedded in this build.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::release::{missed_releases, Release};

/// The `CHANGELOG.md` at the repository root, embedded at compile time: one
/// entry per version, its visible changes and its breaking changes, in English.
/// « What's new » shows its [`embedded_excerpt`].
pub const EMBEDDED_CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// How many versions of the embedded changelog « What's new » shows.
pub const EMBEDDED_VERSIONS: usize = 10;

/// The newest `versions` sections (`## …`) of a changelog, without its title,
/// its preamble and its `## Unreleased` entry: the fallback body of « What's
/// new » when the release list is unavailable, and the body shown to an
/// up-to-date daemon. The older history stays in the file.
pub fn embedded_excerpt(changelog: &str, versions: usize) -> String {
    let mut sections: Vec<String> = Vec::new();
    let mut unreleased = false;
    for line in changelog.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            unreleased = is_unreleased(heading);
            if unreleased {
                continue;
            }
            if sections.len() == versions {
                break;
            }
            sections.push(String::new());
        }
        if unreleased {
            continue;
        }
        if let Some(current) = sections.last_mut() {
            current.push_str(line);
            current.push('\n');
        }
    }
    sections.concat().trim_end().to_string()
}

/// `Unreleased` or `Unreleased · <title>`: the entry the changes not released
/// yet are written under, renamed to their version by `make release-bump`.
fn is_unreleased(heading: &str) -> bool {
    heading == "Unreleased" || heading.starts_with("Unreleased · ")
}

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

/// One markdown document from the missed releases: `## vX.Y.Z · <date>` (a title,
/// not a link), then the notes verbatim.
pub fn render_releases_markdown(missed: &[Release]) -> String {
    let mut md = String::new();
    for r in missed {
        let version = format!("v{}", r.version);
        match r.published_at.as_deref() {
            Some(at) => {
                let date = at.split('T').next().unwrap_or(at);
                md.push_str(&format!("## {version} · {date}\n\n"));
            }
            None => md.push_str(&format!("## {version}\n\n")),
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
            markdown: embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS),
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
        markdown: embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(v: &str, body: &str) -> Release {
        Release {
            version: v.to_string(),
            published_at: Some(format!("2026-09-0{}T10:00:00Z", v.len() % 9 + 1)),
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
        let h60 = c.markdown.find("## v1.60.0 · 2026-09-").unwrap();
        let h59 = c.markdown.find("## v1.59.0 · 2026-09-").unwrap();
        assert!(h60 < h59, "newest first:\n{}", c.markdown);
        assert!(!c.markdown.contains("v1.58.1"));
        assert!(
            !c.markdown.contains("]("),
            "a release title is text, not a link"
        );
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
        assert_eq!(
            c.markdown,
            embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS)
        );

        let f = embedded_changelog("1.58.1", None, "The update check is off.");
        assert_eq!(f.source, ChangelogSource::Embedded);
        assert_eq!(
            f.fallback_reason.as_deref(),
            Some("The update check is off.")
        );
        assert_eq!(f.latest_version, None);
        assert_eq!(
            f.markdown,
            embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS)
        );
    }

    #[test]
    fn the_excerpt_keeps_the_newest_versions_without_the_preamble() {
        let md =
            "# Changelog\n\nPreamble.\n\n## 3.0.0\n\n- c\n\n## 2.0.0\n\n- b\n\n## 1.0.0\n\n- a\n";
        assert_eq!(
            embedded_excerpt(md, 2),
            "## 3.0.0\n\n- c\n\n## 2.0.0\n\n- b"
        );
        assert_eq!(
            embedded_excerpt(md, 9),
            md["# Changelog\n\nPreamble.\n\n".len()..].trim_end()
        );
        assert_eq!(embedded_excerpt("# Changelog\n", 5), "");
    }

    #[test]
    fn the_excerpt_hides_the_unreleased_entry() {
        let md =
            "# Changelog\n\n## Unreleased · Next\n\n- soon\n\n## 2.0.0\n\n- b\n\n## 1.0.0\n\n- a\n";
        assert_eq!(
            embedded_excerpt(md, 2),
            "## 2.0.0\n\n- b\n\n## 1.0.0\n\n- a"
        );
        assert_eq!(embedded_excerpt(md, 1), "## 2.0.0\n\n- b");
        assert_eq!(
            embedded_excerpt("# Changelog\n\n## Unreleased\n\n- soon\n", 5),
            ""
        );
    }

    /// The headings of a changelog: `Unreleased` first if present, then one
    /// entry per version, the newest being `version`. The fault, if any.
    fn head_fault(changelog: &str, version: &str) -> Option<String> {
        let headings: Vec<&str> = changelog
            .lines()
            .filter_map(|l| l.strip_prefix("## "))
            .collect();
        let versions = match headings.split_first() {
            Some((first, rest)) if is_unreleased(first) => rest,
            _ => &headings[..],
        };
        if let Some(stray) = versions.iter().find(|h| is_unreleased(h)) {
            return Some(format!("`## {stray}` is not the first entry"));
        }
        let newest = versions.first().map(|h| h.split(' ').next().unwrap_or(h));
        (newest != Some(version)).then(|| format!("newest entry {newest:?}, build {version}"))
    }

    #[test]
    fn an_unreleased_entry_may_open_the_changelog_above_the_current_version() {
        let without = "# Changelog\n\n## 2.1.0 · Title\n\n- b\n\n## 2.0.0\n";
        let with = "# Changelog\n\n## Unreleased\n\n- c\n\n## 2.1.0 · Title\n\n- b\n";
        assert_eq!(head_fault(without, "2.1.0"), None);
        assert_eq!(head_fault(with, "2.1.0"), None);
        assert!(head_fault(with, "2.2.0").is_some());
        let below = "## 2.1.0\n\n- b\n\n## Unreleased\n\n- c\n";
        assert!(head_fault(below, "2.1.0")
            .unwrap()
            .contains("not the first"));
    }

    /// « What's new » reads one entry per version, the newest being this build's
    /// (an `Unreleased` entry may sit above it), and the changelog has an entry
    /// for every minor since 2.59.
    #[test]
    fn the_changelog_has_one_entry_per_version_up_to_this_build() {
        assert_eq!(
            head_fault(EMBEDDED_CHANGELOG, env!("CARGO_PKG_VERSION")),
            None
        );
        let shown = embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS);
        assert_eq!(
            shown
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("## "))
                .and_then(|h| h.split(' ').next()),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            shown.lines().filter(|l| l.starts_with("## ")).count(),
            EMBEDDED_VERSIONS
        );
        let headings: Vec<&str> = EMBEDDED_CHANGELOG
            .lines()
            .filter_map(|l| l.strip_prefix("## "))
            .map(|h| h.split(' ').next().unwrap_or(h))
            .collect();
        let current: u32 = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
        for minor in 59..=current {
            let version = format!("2.{minor}.0");
            assert!(
                headings.contains(&version.as_str()),
                "no entry for {version}: {headings:?}"
            );
        }
    }

    /// The lines of a changelog that cite a ticket or an issue, or are not in
    /// English, `Unreleased` included.
    fn language_faults(changelog: &str) -> Vec<String> {
        let mut faults = Vec::new();
        for (n, line) in changelog.lines().enumerate() {
            let ticket = line
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .any(|word| {
                    let mut parts = word.splitn(2, '-');
                    let (prefix, number) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    // Standard names share the shape of an id: `UTF-8`, `SHA-256`…
                    !["UTF", "SHA", "ISO", "RFC", "HTTP", "TLS"].contains(&prefix)
                        && prefix.len() >= 2
                        && prefix.chars().all(|c| c.is_ascii_uppercase())
                        && !number.is_empty()
                        && number.chars().all(|c| c.is_ascii_digit())
                });
            if ticket {
                faults.push(format!("line {}: ticket id: {line}", n + 1));
            }
            let issue = line.match_indices('#').any(|(i, _)| {
                line[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
                    && !line[..i].ends_with('&')
            });
            if issue {
                faults.push(format!("line {}: issue number: {line}", n + 1));
            }
            if line.contains(['é', 'è', 'ê', 'à', 'â', 'ç', '«', '»']) {
                faults.push(format!("line {}: not English: {line}", n + 1));
            }
        }
        faults
    }

    /// The changelog is read in the UI: in English, and in the user's terms
    /// rather than the backlog's (no ticket id).
    #[test]
    fn the_changelog_is_in_english_and_cites_no_ticket() {
        assert_eq!(language_faults(EMBEDDED_CHANGELOG), Vec::<String>::new());
    }

    #[test]
    fn an_unreleased_entry_follows_the_same_rules() {
        let md = "## Unreleased\n\n- Faster gates (CORE-158)\n- Fixed #12\n- Plus rapide, déjà\n\n## 2.0.0\n";
        let faults = language_faults(md);
        assert_eq!(faults.len(), 3, "{faults:?}");
        assert!(faults[0].starts_with("line 3: ticket id"));
        assert!(faults[1].starts_with("line 4: issue number"));
        assert!(faults[2].starts_with("line 5: not English"));
        assert_eq!(
            language_faults("## Unreleased\n\n- Faster gates, UTF-8 names\n"),
            Vec::<String>::new()
        );
    }

    /// The UI names no product but its brand: the excerpt « What's new » shows
    /// says « PDO » only in identifiers (`PDO_TOKEN`, `pdo-extension-api`).
    #[test]
    fn the_shown_excerpt_does_not_name_the_old_product() {
        let shown = embedded_excerpt(EMBEDDED_CHANGELOG, EMBEDDED_VERSIONS);
        assert!(shown.starts_with("## "), "{shown}");
        for (i, _) in shown.match_indices("PDO") {
            let before = shown[..i].chars().next_back();
            let after = shown[i + 3..].chars().next();
            let identifier = |c: Option<char>| matches!(c, Some(c) if c == '_' || c == '-' || c.is_alphanumeric());
            assert!(
                identifier(before) || identifier(after),
                "« PDO » shown in What's new: {}",
                shown[..i]
                    .chars()
                    .rev()
                    .take(40)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .chain(shown[i..].chars().take(40))
                    .collect::<String>()
            );
        }
    }
}
