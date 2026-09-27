//! What `GET /update` says (#697, #699): the cached version check, the setting,
//! and whether an update may start — assembled by a pure function of them.

use super::attempt::{apply_gate, UpdateAttempt};
use super::install::{InstallMethod, Supervision};
use super::release::{is_newer, source_label};
use super::update_check::UpdateCache;

/// The daemon's version-check state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub installed_version: String,
    /// `None` when unknown, unreachable at last check, or the check is off;
    /// `reason` then says which. Never a false "up to date".
    pub latest_version: Option<String>,
    pub newer_available: bool,
    pub checked_at: Option<String>,
    /// Human label of the source.
    pub source: String,
    pub source_url: String,
    pub check_enabled: bool,
    pub install_method: InstallMethod,
    pub manual_command: &'static str,
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
    pub installed: &'a str,
    pub enabled: bool,
    pub cache: Option<&'a UpdateCache>,
    pub source_url: &'a str,
    pub method: InstallMethod,
    pub supervision: Supervision,
    pub active_runs: usize,
    pub last_attempt: Option<UpdateAttempt>,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Assemble the status. When the check is off, `latest_version` is `None`
/// whatever the cache holds — never a stale claim the user asked us not to make.
pub fn update_status(inputs: StatusInputs<'_>) -> UpdateStatus {
    let StatusInputs {
        installed,
        enabled,
        cache,
        source_url,
        method,
        supervision,
        active_runs,
        last_attempt,
        now,
    } = inputs;
    let (can_apply, apply_blocked_reason) = apply_gate(method, last_attempt.as_ref(), now);
    let (latest, checked_at, error, reason) = match (enabled, cache) {
        (false, c) => (
            None,
            c.and_then(|c| c.checked_at.clone()),
            None,
            Some("Update check is off."),
        ),
        (true, None) => (None, None, None, Some("Not checked yet.")),
        (true, Some(c)) => {
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
        installed_version: installed.to_string(),
        latest_version: latest,
        newer_available,
        checked_at,
        source: source_label(source_url),
        source_url: source_url.to_string(),
        check_enabled: enabled,
        install_method: method,
        manual_command: method.manual_command(),
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
            installed: "1.0.0",
            enabled,
            cache,
            source_url: "http://127.0.0.1:9/latest",
            method: InstallMethod::Script,
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
        assert_eq!(s.source, "127.0.0.1:9");
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
}
