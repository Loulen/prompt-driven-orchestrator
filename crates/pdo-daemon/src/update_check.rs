//! Version check against the release source (#697, story #695).
//!
//! The **daemon** — never the browser — asks GitHub Releases which version is the
//! latest published one, at boot, every [`UPDATE_CHECK_INTERVAL`] and on demand,
//! and caches the answer on disk with its date. The cache is what every read of
//! `GET /update` and the status-bar badge reflects: a page load never triggers a
//! request. The `update_check` instance setting (default **on**) cuts this egress
//! entirely; off, no request ever leaves the daemon (CONTEXT.md § *Mise à jour
//! depuis l'app*, *Mono-user, local*: egress opt-out and failure-tolerant).
//!
//! Also here, as **pure functions**: the installation-method detection (Homebrew
//! Cellar / cargo-dist receipt / unknown) with the manual command each implies,
//! and the supervision detection (systemd / launchd / none) from the environment
//! the supervisor sets. "Unknown" is declared, never guessed (ADR-0045): a future
//! Update button will be absent with the reason shown, not fall back to a guess.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The default release source: the GitHub Releases API, latest non-prerelease.
pub(crate) const RELEASE_SOURCE_URL_DEFAULT: &str =
    "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest";

/// Human label for the default source, shown next to the check date.
pub(crate) const RELEASE_SOURCE_LABEL_DEFAULT: &str = "GitHub Releases";

/// Interval between two periodic checks. The boot check and the periodic loop
/// both skip when the cache is younger than this — that is the "cache" AC.
pub(crate) const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// Hard timeout on the release fetch; a slow GitHub must never hold anything.
pub(crate) const UPDATE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Env seam for the `update_check` setting (`stored → env → default(true)`).
pub const UPDATE_CHECK_ENV: &str = "PDO_UPDATE_CHECK";

/// Built-in default: the check is **on**. Opt-out egress, like the others.
pub const UPDATE_CHECK_DEFAULT: bool = true;

/// Optional GitHub token to lift the unauthenticated rate limit. Env only,
/// deliberately not a setting.
pub(crate) const GITHUB_TOKEN_ENV: &str = "PDO_GITHUB_TOKEN";

/// Schema tag of the on-disk cache, so a future shape change is detected.
pub(crate) const CACHE_SCHEMA: &str = "update-check-v1";

/// Homebrew formula the tap publishes; the manual command names it in full so
/// `brew` never resolves an ambiguous `pdo`.
pub(crate) const HOMEBREW_COMMAND: &str = "brew update && brew upgrade Loulen/tap/pdo";
/// Re-running the cargo-dist installer replaces `~/.local/bin/pdo` the supported way.
pub(crate) const SCRIPT_COMMAND: &str = "curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-installer.sh | sh";
/// No installer to delegate to: the user rebuilds, PDO does not update itself.
pub(crate) const UNKNOWN_COMMAND: &str = "Build from source, then restart the daemon.";

/// The `env` tier of the `update_check` setting.
pub fn env_update_check() -> Option<bool> {
    std::env::var(UPDATE_CHECK_ENV)
        .ok()
        .as_deref()
        .and_then(crate::stale_detector::parse_bool_setting)
}

/// Resolve `update_check`: `stored → env → default(true)` (ADR-0015). A stored
/// `0` is a decision that beats the env, hence `Option<i64>` on the stored tier.
pub fn update_check_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_update_check().unwrap_or(UPDATE_CHECK_DEFAULT),
    }
}

// ------------------------------------------------------------------------------------
// Installation method
// ------------------------------------------------------------------------------------

/// How this binary got here — what a future Update delegates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMethod {
    Homebrew,
    Script,
    Unknown,
}

impl InstallMethod {
    /// The exact command the user would type — and the future Update button will run.
    pub fn manual_command(self) -> &'static str {
        match self {
            InstallMethod::Homebrew => HOMEBREW_COMMAND,
            InstallMethod::Script => SCRIPT_COMMAND,
            InstallMethod::Unknown => UNKNOWN_COMMAND,
        }
    }
}

/// Pure detection over (resolved binary path, cargo-dist receipt presence, Homebrew
/// prefix). Order matters: the Cellar is the strongest evidence (the path IS the
/// formula's), the receipt only says the script ran once; `unknown` otherwise.
pub fn detect_install_method(
    exe_path: &Path,
    receipt_exists: bool,
    brew_prefix: Option<&Path>,
) -> InstallMethod {
    let components: Vec<&str> = exe_path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let under_cellar = components.contains(&"Cellar");
    let under_prefix = brew_prefix
        .filter(|p| !p.as_os_str().is_empty())
        .is_some_and(|p| exe_path.starts_with(p));
    if under_cellar || under_prefix {
        return InstallMethod::Homebrew;
    }
    if receipt_exists {
        return InstallMethod::Script;
    }
    InstallMethod::Unknown
}

/// Where cargo-dist writes its install receipt: `$XDG_CONFIG_HOME/pdo/pdo-receipt.json`
/// (falling back to `~/.config/pdo/`).
pub(crate) fn receipt_path(config_home: &Path) -> PathBuf {
    config_home.join("pdo").join("pdo-receipt.json")
}

/// Impure wrapper: read the environment once and feed the pure detector.
pub(crate) fn detect_install_method_from_env() -> InstallMethod {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .unwrap_or_default();
    let receipt = crate::service_unit::resolve_config_home()
        .map(|h| receipt_path(&h).is_file())
        .unwrap_or(false);
    let prefix = std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from);
    detect_install_method(&exe, receipt, prefix.as_deref())
}

// ------------------------------------------------------------------------------------
// Supervision
// ------------------------------------------------------------------------------------

/// Who restarts the daemon after an update — or nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Supervision {
    Systemd,
    Launchd,
    None,
}

/// Pure detection from the environment a supervisor sets on its children:
/// `INVOCATION_ID` (systemd), `XPC_SERVICE_NAME` (launchd, not the shell's
/// `0` placeholder). Empty values count as absent.
pub fn detect_supervision(
    invocation_id: Option<&str>,
    xpc_service_name: Option<&str>,
) -> Supervision {
    if invocation_id.is_some_and(|v| !v.trim().is_empty()) {
        return Supervision::Systemd;
    }
    if xpc_service_name.is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && v != "0"
    }) {
        return Supervision::Launchd;
    }
    Supervision::None
}

/// Impure wrapper over [`detect_supervision`].
pub(crate) fn detect_supervision_from_env() -> Supervision {
    detect_supervision(
        std::env::var("INVOCATION_ID").ok().as_deref(),
        std::env::var("XPC_SERVICE_NAME").ok().as_deref(),
    )
}

// ------------------------------------------------------------------------------------
// Versions
// ------------------------------------------------------------------------------------

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

/// GET the release source, bounded by [`UPDATE_FETCH_TIMEOUT`]. GitHub requires a
/// `User-Agent`; a `PDO_GITHUB_TOKEN` lifts the anonymous rate limit when set.
pub(crate) async fn fetch_latest(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(UPDATE_FETCH_TIMEOUT)
        .user_agent(format!("pdo/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .get(url)
        .header("accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var(GITHUB_TOKEN_ENV) {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    parse_release_body(&body)
}

// ------------------------------------------------------------------------------------
// Changelog (#698)
// ------------------------------------------------------------------------------------

/// The `CHANGELOG.md` at the repository root, embedded at compile time: the
/// fallback body of « What's new » when the release list is unavailable, and the
/// body shown to an up-to-date daemon. It lists breaking changes only.
pub const EMBEDDED_CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

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

/// Where the changelog lives, or why it could not come from the releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangelogSource {
    /// Assembled from the release notes strictly newer than the installed version.
    Releases,
    /// The `CHANGELOG.md` embedded in this build.
    Embedded,
}

/// The assembled « What's new » payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// GET the releases list, bounded by [`UPDATE_FETCH_TIMEOUT`], same headers as
/// [`fetch_latest`].
pub(crate) async fn fetch_releases(url: &str) -> Result<Vec<Release>, String> {
    let client = reqwest::Client::builder()
        .timeout(UPDATE_FETCH_TIMEOUT)
        .user_agent(format!("pdo/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .get(url)
        .header("accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var(GITHUB_TOKEN_ENV) {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    parse_releases_body(&body)
}

// ------------------------------------------------------------------------------------
// Cache
// ------------------------------------------------------------------------------------

/// The on-disk (and in-memory) result of the last check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateCache {
    #[serde(default)]
    pub schema: String,
    /// The source URL the check hit.
    #[serde(default)]
    pub source: String,
    /// RFC3339 UTC date of the last check — success **or** failure.
    #[serde(default)]
    pub checked_at: Option<String>,
    /// Last successfully fetched version; kept across a failed check.
    #[serde(default)]
    pub latest_version: Option<String>,
    /// Why the last check failed, when it did. `None` after a success.
    #[serde(default)]
    pub error: Option<String>,
}

impl UpdateCache {
    /// Age since `checked_at`, or `None` when never checked / unparseable (a cache
    /// whose vintage cannot be read is one worth refreshing).
    pub fn age(&self) -> Option<chrono::Duration> {
        let at = chrono::DateTime::parse_from_rfc3339(self.checked_at.as_deref()?).ok()?;
        Some(chrono::Utc::now().signed_duration_since(at.with_timezone(&chrono::Utc)))
    }

    /// `true` when a periodic pass should skip: checked within the interval.
    pub fn is_fresh(&self, interval: Duration) -> bool {
        match self.age() {
            Some(age) => {
                age < chrono::Duration::from_std(interval)
                    .unwrap_or_else(|_| chrono::Duration::hours(6))
            }
            None => false,
        }
    }
}

/// `<home>/.pdo/update/check.json`.
pub(crate) fn cache_path(home_root: &Path) -> PathBuf {
    home_root.join(".pdo").join("update").join("check.json")
}

/// Read the cache, `None` when absent or unreadable (a corrupt cache is simply "not
/// checked yet", never an error).
pub(crate) fn read_cache(path: &Path) -> Option<UpdateCache> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: UpdateCache = serde_json::from_str(&text).ok()?;
    (cache.schema == CACHE_SCHEMA).then_some(cache)
}

/// Write the cache atomically (temp + rename), creating the directory.
pub(crate) fn write_cache(path: &Path, cache: &UpdateCache) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(cache).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Label a source URL for the UI: the shipped default reads "GitHub Releases", an
/// override reads as its host so a test fixture is recognisable.
pub fn source_label(url: &str) -> String {
    if url == RELEASE_SOURCE_URL_DEFAULT {
        return RELEASE_SOURCE_LABEL_DEFAULT.to_string();
    }
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cellar_path_is_homebrew() {
        let p = Path::new("/home/linuxbrew/.linuxbrew/Cellar/pdo/1.58.1/bin/pdo");
        assert_eq!(
            detect_install_method(p, false, None),
            InstallMethod::Homebrew
        );
        let mac = Path::new("/opt/homebrew/Cellar/pdo/1.58.1/bin/pdo");
        assert_eq!(
            detect_install_method(mac, true, None),
            InstallMethod::Homebrew
        );
    }

    #[test]
    fn brew_prefix_match_is_homebrew_even_without_cellar() {
        let p = Path::new("/opt/homebrew/bin/pdo");
        assert_eq!(
            detect_install_method(p, false, Some(Path::new("/opt/homebrew"))),
            InstallMethod::Homebrew
        );
        // An empty prefix must not match everything.
        assert_eq!(
            detect_install_method(Path::new("/usr/local/bin/pdo"), false, Some(Path::new(""))),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn receipt_present_is_script() {
        let p = Path::new("/home/u/.local/bin/pdo");
        assert_eq!(detect_install_method(p, true, None), InstallMethod::Script);
    }

    #[test]
    fn otherwise_unknown() {
        let p = Path::new("/home/u/.local/bin/pdo");
        assert_eq!(
            detect_install_method(p, false, None),
            InstallMethod::Unknown
        );
        assert_eq!(
            detect_install_method(p, false, Some(Path::new("/opt/homebrew"))),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn manual_command_matches_the_method() {
        assert_eq!(
            InstallMethod::Homebrew.manual_command(),
            "brew update && brew upgrade Loulen/tap/pdo"
        );
        assert!(InstallMethod::Script
            .manual_command()
            .contains("pdo-installer.sh"));
        assert!(InstallMethod::Unknown
            .manual_command()
            .starts_with("Build from source"));
    }

    #[test]
    fn supervision_from_env() {
        assert_eq!(
            detect_supervision(Some("abc123"), None),
            Supervision::Systemd
        );
        assert_eq!(
            detect_supervision(None, Some("com.pdo.daemon")),
            Supervision::Launchd
        );
        // The shell's `XPC_SERVICE_NAME=0` placeholder is not launchd.
        assert_eq!(detect_supervision(None, Some("0")), Supervision::None);
        assert_eq!(detect_supervision(Some(""), Some("")), Supervision::None);
        assert_eq!(detect_supervision(None, None), Supervision::None);
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
    fn cache_round_trips_and_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(dir.path());
        assert!(read_cache(&path).is_none());
        let cache = UpdateCache {
            schema: CACHE_SCHEMA.to_string(),
            source: "http://x".into(),
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            latest_version: Some("1.59.0".into()),
            error: None,
        };
        write_cache(&path, &cache).unwrap();
        assert_eq!(read_cache(&path), Some(cache.clone()));
        assert!(cache.is_fresh(UPDATE_CHECK_INTERVAL));
        let old = UpdateCache {
            checked_at: Some((chrono::Utc::now() - chrono::Duration::hours(7)).to_rfc3339()),
            ..cache
        };
        assert!(!old.is_fresh(UPDATE_CHECK_INTERVAL));
        assert!(!UpdateCache::default().is_fresh(UPDATE_CHECK_INTERVAL));
        // A foreign schema reads as "never checked".
        std::fs::write(&path, r#"{"schema":"other"}"#).unwrap();
        assert!(read_cache(&path).is_none());
    }

    #[test]
    fn source_labels() {
        assert_eq!(source_label(RELEASE_SOURCE_URL_DEFAULT), "GitHub Releases");
        assert_eq!(
            source_label("http://127.0.0.1:4321/latest"),
            "127.0.0.1:4321"
        );
    }

    #[test]
    fn update_check_resolves_stored_over_default() {
        assert!(update_check_with(Some(1)));
        assert!(!update_check_with(Some(0)));
    }

    fn rel(v: &str, body: &str) -> Release {
        Release {
            version: v.to_string(),
            published_at: Some(format!("2026-09-0{}T10:00:00Z", v.len() % 9 + 1)),
            html_url: Some(format!(
                "https://github.com/Loulen/prompt-driven-orchestrator/releases/tag/v{v}"
            )),
            body: body.to_string(),
        }
    }

    #[test]
    fn releases_list_url_drops_latest() {
        assert_eq!(
            releases_list_url(RELEASE_SOURCE_URL_DEFAULT),
            "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases"
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
