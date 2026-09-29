//! Versions and published releases (#697, #698): parsing, comparison, and the
//! reading of the release source's payloads. Pure: the fetch is an adapter.

use serde::{Deserialize, Serialize};

/// Human label of a source on the GitHub API, shown next to the check date.
pub const RELEASE_SOURCE_LABEL_GITHUB: &str = "GitHub Releases";

/// The host of the GitHub API, whose sources read as [`RELEASE_SOURCE_LABEL_GITHUB`].
const GITHUB_API_HOST: &str = "api.github.com";

/// Parse `1.58.1`, `v1.58.1` or `pdo-v1.58.1` into a comparable triple; `None` on
/// anything else (a pre-release suffix is dropped, not compared).
pub fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let s = raw.trim();
    let s = s.rsplit_once('v').map(|(_, r)| r).unwrap_or(s);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// `true` when `latest` is strictly newer than `installed`. Unparseable input on
/// either side ⇒ `false`: never a badge on a guess.
pub fn is_newer(installed: &str, latest: &str) -> bool {
    match (parse_version(installed), parse_version(latest)) {
        (Some(i), Some(l)) => l > i,
        _ => false,
    }
}

/// Extract the version from the GitHub `releases/latest` payload (`tag_name`,
/// leading `v` stripped). A payload without a parseable tag is an error, not a
/// stale `null`: the caller keeps the last good value and reports the reason.
pub fn parse_release_body(body: &str) -> Result<String, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("release payload is not JSON: {e}"))?;
    let tag = doc
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "release payload has no `tag_name`".to_string())?;
    let (maj, min, pat) =
        parse_version(tag).ok_or_else(|| format!("release tag `{tag}` is not a version"))?;
    Ok(format!("{maj}.{min}.{pat}"))
}

/// One published release, trimmed to what the changelog needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Bare `major.minor.patch`, tag prefix stripped.
    pub version: String,
    /// `published_at` as GitHub sends it (RFC3339), when present.
    pub published_at: Option<String>,
    /// The release page, when present.
    pub html_url: Option<String>,
    /// Release notes, verbatim markdown. Empty when the release has none.
    pub body: String,
}

/// The releases-list URL for a `releases/latest` source URL: drop the trailing
/// `/latest` (GitHub: `/repos/{owner}/{repo}/releases`). A source that does not
/// end in `/latest` is used as is.
pub fn releases_list_url(latest_url: &str) -> String {
    let trimmed = latest_url.trim_end_matches('/');
    match trimmed.strip_suffix("/latest") {
        Some(base) => base.to_string(),
        None => trimmed.to_string(),
    }
}

/// Parse the GitHub `releases` list payload. Drafts and pre-releases are dropped,
/// as are entries whose tag is not a version. A payload that is not a JSON array
/// is an error (the caller falls back to the embedded changelog with the reason).
pub fn parse_releases_body(body: &str) -> Result<Vec<Release>, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("releases payload is not JSON: {e}"))?;
    let items = doc
        .as_array()
        .ok_or_else(|| "releases payload is not a JSON array".to_string())?;
    Ok(items
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let (maj, min, pat) = parse_version(tag)?;
            Some(Release {
                version: format!("{maj}.{min}.{pat}"),
                published_at: r["published_at"].as_str().map(str::to_string),
                html_url: r["html_url"].as_str().map(str::to_string),
                body: r["body"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect())
}

/// Keep the releases strictly newer than `installed`, newest first. Unparseable
/// entries (or an unparseable `installed`) yield an empty list: never a claim on a
/// guess.
pub fn missed_releases(installed: &str, releases: &[Release]) -> Vec<Release> {
    let Some(installed) = parse_version(installed) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, u64, u64, Release)> = releases
        .iter()
        .filter_map(|r| {
            let v = parse_version(&r.version)?;
            (v > installed).then(|| (v.0, v.1, v.2, r.clone()))
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse((r.0, r.1, r.2)));
    out.dedup_by(|a, b| (a.0, a.1, a.2) == (b.0, b.1, b.2));
    out.into_iter().map(|(_, _, _, r)| r).collect()
}

/// Label a source URL for the UI: a GitHub API source reads "GitHub Releases",
/// any other reads as its host so a test fixture is recognisable.
pub fn source_label(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|h| !h.is_empty());
    match host {
        Some(GITHUB_API_HOST) => RELEASE_SOURCE_LABEL_GITHUB.to_string(),
        Some(host) => host.to_string(),
        None => url.to_string(),
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
    fn version_parsing_and_comparison() {
        assert_eq!(parse_version("v1.58.1"), Some((1, 58, 1)));
        assert_eq!(parse_version("1.59.0"), Some((1, 59, 0)));
        assert_eq!(parse_version("pdo-v2.0.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.60.0-rc.1"), Some((1, 60, 0)));
        assert_eq!(parse_version("nope"), None);
        assert!(is_newer("1.58.1", "1.59.0"));
        assert!(is_newer("1.58.1", "v2.0.0"));
        assert!(!is_newer("1.58.1", "1.58.1"));
        assert!(!is_newer("1.58.1", "1.57.9"));
        assert!(!is_newer("1.58.1", "garbage"));
    }

    #[test]
    fn release_body_yields_a_bare_version() {
        assert_eq!(
            parse_release_body(r#"{"tag_name":"v1.59.0","name":"x"}"#).unwrap(),
            "1.59.0"
        );
        assert!(parse_release_body(r#"{"name":"x"}"#).is_err());
        assert!(parse_release_body("<html>").is_err());
        assert!(parse_release_body(r#"{"tag_name":"latest"}"#).is_err());
    }

    #[test]
    fn source_labels() {
        assert_eq!(source_label(GITHUB_LATEST), "GitHub Releases");
        assert_eq!(
            source_label("http://127.0.0.1:4321/latest"),
            "127.0.0.1:4321"
        );
        assert_eq!(source_label("not a url"), "not a url");
    }

    const GITHUB_LATEST: &str = "https://api.github.com/repos/Loulen/PDO-v2/releases/latest";

    #[test]
    fn releases_list_url_drops_latest() {
        assert_eq!(
            releases_list_url(GITHUB_LATEST),
            "https://api.github.com/repos/Loulen/PDO-v2/releases"
        );
        assert_eq!(
            releases_list_url("http://127.0.0.1:1/releases/latest/"),
            "http://127.0.0.1:1/releases"
        );
        assert_eq!(releases_list_url("http://x/list"), "http://x/list");
    }

    #[test]
    fn releases_body_drops_drafts_prereleases_and_non_versions() {
        let body = r#"[
          {"tag_name":"v1.60.0","published_at":"2026-09-06T00:00:00Z","html_url":"u1","body":"- a"},
          {"tag_name":"v1.61.0-rc.1","prerelease":true,"body":"rc"},
          {"tag_name":"v1.62.0","draft":true,"body":"draft"},
          {"tag_name":"nightly","body":"x"},
          {"tag_name":"v1.59.0","body":null}
        ]"#;
        let rels = parse_releases_body(body).unwrap();
        assert_eq!(
            rels.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(),
            ["1.60.0", "1.59.0"]
        );
        assert_eq!(
            rels[0].published_at.as_deref(),
            Some("2026-09-06T00:00:00Z")
        );
        assert_eq!(rels[0].html_url.as_deref(), Some("u1"));
        assert_eq!(rels[1].body, "");
        assert!(parse_releases_body(r#"{"tag_name":"v1"}"#).is_err());
        assert!(parse_releases_body("<html>").is_err());
    }

    #[test]
    fn missed_releases_are_strictly_newer_and_sorted_desc() {
        let all = vec![
            rel("1.58.1", "same"),
            rel("1.60.0", "newest"),
            rel("1.57.0", "older"),
            rel("1.59.0", "middle"),
            rel("1.59.0", "duplicate"),
            rel("2.0.0", "major"),
        ];
        let missed = missed_releases("1.58.1", &all);
        assert_eq!(
            missed
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["2.0.0", "1.60.0", "1.59.0"]
        );
        assert!(missed_releases("2.0.0", &all).is_empty());
        assert!(missed_releases("garbage", &all).is_empty());
    }
}
