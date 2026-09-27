//! The version check (#697): whether it may run, and what the last one found.
//!
//! The **daemon** — never the browser — asks the release source which version is
//! the latest published one, at boot, every [`UPDATE_CHECK_INTERVAL`] and on
//! demand, and caches the answer with its date. The cache is what every read of
//! `GET /update` reflects: a page load never triggers a request. The
//! `update_check` instance setting (default **on**) cuts this egress entirely.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Interval between two periodic checks. The boot check and the periodic loop
/// both skip when the cache is younger than this.
pub const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// Env seam for the `update_check` setting (`stored → env → default(true)`).
pub const UPDATE_CHECK_ENV: &str = "PDO_UPDATE_CHECK";

/// Built-in default: the check is **on**. Opt-out egress, like the others.
pub const UPDATE_CHECK_DEFAULT: bool = true;

/// Schema tag of the on-disk cache, so a future shape change is detected.
pub const CACHE_SCHEMA: &str = "update-check-v1";

/// A boolean setting as the environment spells it; `None` when it is not one.
pub fn parse_bool_setting(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Resolve `update_check`: `stored → env → default(true)` (ADR-0015). A stored
/// `0` is a decision that beats the env, hence `Option<i64>` on the stored tier.
pub fn resolve_update_check(stored: Option<i64>, env: Option<bool>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env.unwrap_or(UPDATE_CHECK_DEFAULT),
    }
}

/// The result of the last check, on disk and in memory.
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

    /// Record one check of `source` at `now`: the date moves either way, the
    /// version only on success (a failed check keeps the last good value, with
    /// the error beside it).
    pub fn record(&mut self, source: &str, now: String, outcome: &Result<String, String>) {
        self.schema = CACHE_SCHEMA.to_string();
        self.source = source.to_string();
        self.checked_at = Some(now);
        match outcome {
            Ok(latest) => {
                self.latest_version = Some(latest.clone());
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("release source unreachable: {source}: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_check_resolves_stored_then_env_then_default() {
        assert!(resolve_update_check(Some(1), Some(false)));
        assert!(!resolve_update_check(Some(0), Some(true)));
        assert!(!resolve_update_check(None, Some(false)));
        assert!(resolve_update_check(None, None));
        assert_eq!(parse_bool_setting(" Off "), Some(false));
        assert_eq!(parse_bool_setting("yes"), Some(true));
        assert_eq!(parse_bool_setting("maybe"), None);
    }

    #[test]
    fn freshness_follows_the_interval() {
        let cache = UpdateCache {
            schema: CACHE_SCHEMA.to_string(),
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            ..UpdateCache::default()
        };
        assert!(cache.is_fresh(UPDATE_CHECK_INTERVAL));
        let old = UpdateCache {
            checked_at: Some((chrono::Utc::now() - chrono::Duration::hours(7)).to_rfc3339()),
            ..cache
        };
        assert!(!old.is_fresh(UPDATE_CHECK_INTERVAL));
        assert!(!UpdateCache::default().is_fresh(UPDATE_CHECK_INTERVAL));
    }

    #[test]
    fn a_failed_check_keeps_the_last_good_version() {
        let mut cache = UpdateCache::default();
        cache.record("http://x", "t1".into(), &Ok("1.2.3".into()));
        assert_eq!(cache.latest_version.as_deref(), Some("1.2.3"));
        assert_eq!(cache.error, None);
        cache.record("http://x", "t2".into(), &Err("HTTP 500".into()));
        assert_eq!(cache.latest_version.as_deref(), Some("1.2.3"));
        assert_eq!(cache.checked_at.as_deref(), Some("t2"));
        assert_eq!(
            cache.error.as_deref(),
            Some("release source unreachable: http://x: HTTP 500")
        );
        assert_eq!(cache.schema, CACHE_SCHEMA);
    }
}
