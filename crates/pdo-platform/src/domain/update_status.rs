//! What `GET /update` says (#697, #699): the cached version check, the setting,
//! and whether an update may start — assembled by a pure function of them.

use super::attempt::{channel_apply_gate, UpdateAttempt};
use super::install::{InstallMethod, Supervision};
use super::release::{is_newer, source_label, source_page};
use super::update_check::UpdateCache;

/// Why there is no latest version when the release channel declares no source.
pub const NO_SOURCE_REASON: &str = "No release source declared: no version check.";

/// The daemon's version-check state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    /// The release channel's name (`PDO` for the vanilla binary, #24).
    pub channel_name: String,
    /// The version the channel declares, compared with the releases.
    pub installed_version: String,
    /// The version of the core the binary is built on.
    pub core_version: String,
    /// `None` when unknown, unreachable at last check, the check is off, or
    /// there is no source; `reason` then says which. Never a false "up to date".
    pub latest_version: Option<String>,
    pub newer_available: bool,
    pub checked_at: Option<String>,
    /// Human label of the source; `None` without a source.
    pub source: Option<String>,
    pub source_url: Option<String>,
    /// The page a human opens for the source (a GitHub API source: its
    /// repository's releases page); `None` without a source.
    pub source_page_url: Option<String>,
    pub check_enabled: bool,
    pub install_method: InstallMethod,
    /// What an Update runs; `None` when the channel declares no command.
    pub manual_command: Option<String>,
    pub supervision: Supervision,
    pub reason: Option<&'static str>,
    pub last_error: Option<String>,
    /// Live Runs — informational only: an update is never refused for them.
    pub active_runs: usize,
    pub can_apply: bool,
    pub apply_blocked_reason: Option<String>,
    pub last_attempt: Option<UpdateAttempt>,
}

/// What the status is assembled from.
pub struct StatusInputs<'a> {
    pub channel_name: &'a str,
    pub installed: &'a str,
    pub core_version: &'a str,
    pub enabled: bool,
    pub cache: Option<&'a UpdateCache>,
    /// `None`: the channel declares no source, so nothing is checked.
    pub source_url: Option<&'a str>,
    pub method: InstallMethod,
    pub manual_command: Option<&'a str>,
    pub supervision: Supervision,
    pub active_runs: usize,
    pub last_attempt: Option<UpdateAttempt>,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Assemble the status. When the check is off, `latest_version` is `None`
/// whatever the cache holds — never a stale claim the user asked us not to make.
/// Without a source, or with a cache written for another source, there is no
/// answer either.
pub fn update_status(inputs: StatusInputs<'_>) -> UpdateStatus {
    let StatusInputs {
        channel_name,
        installed,
        core_version,
        enabled,
        cache,
        source_url,
        method,
        manual_command,
        supervision,
        active_runs,
        last_attempt,
        now,
    } = inputs;
    let (can_apply, apply_blocked_reason) = channel_apply_gate(
        channel_name,
        method,
        manual_command,
        last_attempt.as_ref(),
        now,
    );
    let cache = match source_url {
        Some(url) => cache.filter(|c| c.is_for(url)),
        None => None,
    };
    let (latest, checked_at, error, reason) = match (source_url, enabled, cache) {
        (None, _, _) => (None, None, None, Some(NO_SOURCE_REASON)),
        (Some(_), false, c) => (
            None,
            c.and_then(|c| c.checked_at.clone()),
            None,
            Some("Update check is off."),
        ),
        (Some(_), true, None) => (None, None, None, Some("Not checked yet.")),
        (Some(_), true, Some(c)) => {
            let reason = if c.latest_version.is_none() {
                if c.error.is_some() {
                    Some("Release source unreachable at last check.")
                } else {
                    Some("Not checked yet.")
                }
            } else {
                None
            };
            (
                c.latest_version.clone(),
                c.checked_at.clone(),
                c.error.clone(),
                reason,
            )
        }
    };
    let newer_available = latest.as_deref().is_some_and(|l| is_newer(installed, l));
    UpdateStatus {
        channel_name: channel_name.to_string(),
        installed_version: installed.to_string(),
        core_version: core_version.to_string(),
        latest_version: latest,
        newer_available,
        checked_at,
        source: source_url.map(source_label),
        source_url: source_url.map(str::to_string),
        source_page_url: source_url.map(source_page),
        check_enabled: enabled,
        install_method: method,
        manual_command: manual_command.map(str::to_string),
        supervision,
        reason,
        last_error: error,
        active_runs,
        can_apply,
        apply_blocked_reason,
        last_attempt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(enabled: bool, cache: Option<&UpdateCache>) -> StatusInputs<'_> {
        StatusInputs {
            channel_name: "PDO",
            installed: "1.0.0",
            core_version: "1.0.0",
            enabled,
            cache,
            source_url: Some("http://127.0.0.1:9/latest"),
            method: InstallMethod::Script,
            manual_command: InstallMethod::Script.manual_command(),
            supervision: Supervision::None,
            active_runs: 2,
            last_attempt: None,
            now: chrono::Utc::now(),
        }
    }

    #[test]
    fn off_hides_the_cached_version_and_says_why() {
        let cache = UpdateCache {
            checked_at: Some("t".into()),
            latest_version: Some("2.0.0".into()),
            ..UpdateCache::default()
        };
        let s = update_status(inputs(false, Some(&cache)));
        assert_eq!(s.latest_version, None);
        assert_eq!(s.checked_at.as_deref(), Some("t"));
        assert_eq!(s.reason, Some("Update check is off."));
        assert!(!s.newer_available);
        assert_eq!(s.source.as_deref(), Some("127.0.0.1:9"));
    }

    #[test]
    fn on_reports_the_cache_and_the_gap() {
        assert_eq!(
            update_status(inputs(true, None)).reason,
            Some("Not checked yet.")
        );
        let cache = UpdateCache {
            latest_version: Some("2.0.0".into()),
            ..UpdateCache::default()
        };
        let s = update_status(inputs(true, Some(&cache)));
        assert!(s.newer_available);
        assert_eq!(s.reason, None);
        assert!(s.can_apply);
        assert_eq!(s.active_runs, 2);
        let failed = UpdateCache {
            error: Some("down".into()),
            ..UpdateCache::default()
        };
        let s = update_status(inputs(true, Some(&failed)));
        assert_eq!(s.reason, Some("Release source unreachable at last check."));
        assert_eq!(s.last_error.as_deref(), Some("down"));
    }

    #[test]
    fn a_distribution_reports_its_channel_and_the_core() {
        let cache = UpdateCache {
            source: "http://127.0.0.1:9/latest".into(),
            latest_version: Some("0.4.0".into()),
            ..UpdateCache::default()
        };
        let s = update_status(StatusInputs {
            channel_name: "acme",
            installed: "0.3.0",
            core_version: "2.0.0",
            method: InstallMethod::Declared,
            manual_command: Some("acme-upgrade"),
            ..inputs(true, Some(&cache))
        });
        assert_eq!(s.channel_name, "acme");
        assert_eq!(s.installed_version, "0.3.0");
        assert_eq!(s.core_version, "2.0.0");
        assert!(s.newer_available);
        assert_eq!(s.manual_command.as_deref(), Some("acme-upgrade"));
        assert!(s.can_apply);
    }

    #[test]
    fn no_command_closes_the_gate_and_no_source_answers_nothing() {
        let cache = UpdateCache {
            latest_version: Some("9.0.0".into()),
            checked_at: Some("t".into()),
            ..UpdateCache::default()
        };
        let s = update_status(StatusInputs {
            channel_name: "acme",
            method: InstallMethod::Declared,
            manual_command: None,
            source_url: None,
            ..inputs(true, Some(&cache))
        });
        assert!(!s.can_apply);
        assert!(s.apply_blocked_reason.is_some_and(|r| !r.is_empty()));
        assert_eq!(s.manual_command, None);
        // No source: the cache (whoever wrote it) is not an answer.
        assert_eq!(s.latest_version, None);
        assert_eq!(s.checked_at, None);
        assert_eq!(s.reason, Some(NO_SOURCE_REASON));
        assert_eq!(s.source, None);
        assert_eq!(s.source_url, None);
    }

    #[test]
    fn a_cache_written_for_another_source_is_not_an_answer() {
        let other = UpdateCache {
            source: "https://api.github.com/repos/someone/else/releases/latest".into(),
            latest_version: Some("9.0.0".into()),
            ..UpdateCache::default()
        };
        let s = update_status(inputs(true, Some(&other)));
        assert_eq!(s.latest_version, None);
        assert!(!s.newer_available);
        assert_eq!(s.reason, Some("Not checked yet."));
    }
}
