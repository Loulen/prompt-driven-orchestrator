//! An update attempt (#699): the journal the detached executor leaves behind, so
//! a failed update is diagnosable in Settings afterwards, and the gate that says
//! whether a new one may start.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::install::{InstallMethod, Supervision};

/// Schema tag of `last-attempt.json`.
pub const ATTEMPT_SCHEMA: &str = "update-attempt-v1";

/// A `running` attempt is considered stale — the daemon it belonged to is gone or
/// the executor crashed without recording — after this long. Past it, apply is
/// allowed again.
pub const ATTEMPT_STALE_AFTER: Duration = Duration::from_secs(30 * 60);

/// Outcome of an attempt, as the record on disk reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "UpdateAttemptStatus")]
#[serde(rename_all = "lowercase")]
pub enum AttemptStatus {
    /// Spawned; the daemon (or the script) has not recorded an end yet.
    Running,
    Succeeded,
    Failed,
}

/// The last update attempt — what Settings › Version & update shows afterwards,
/// including in failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateAttempt {
    #[serde(default)]
    pub schema: String,
    pub attempt_id: String,
    pub status: AttemptStatus,
    /// RFC3339 UTC.
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub method: InstallMethod,
    /// The upgrade command the executor ran (verbatim).
    pub command: String,
    pub supervision: Supervision,
    pub log_path: PathBuf,
    /// Version the daemon ran when the attempt started — so the UI can tell « the
    /// daemon came back on the same version » from a real update.
    #[serde(default)]
    pub from_version: String,
}

impl UpdateAttempt {
    /// Record the exit of the executor: its code, the end date if the script did
    /// not write one, and the outcome the code means.
    pub fn finish(&mut self, exit_code: i32, now: impl FnOnce() -> String) {
        self.exit_code = Some(exit_code);
        self.finished_at.get_or_insert_with(now);
        self.status = if exit_code == 0 {
            AttemptStatus::Succeeded
        } else {
            AttemptStatus::Failed
        };
    }
}

/// Is `attempt` still running at `now`? A `running` record older than
/// [`ATTEMPT_STALE_AFTER`], or with an unreadable start, is not.
pub fn attempt_is_running(attempt: &UpdateAttempt, now: chrono::DateTime<chrono::Utc>) -> bool {
    if attempt.status != AttemptStatus::Running {
        return false;
    }
    let started = chrono::DateTime::parse_from_rfc3339(&attempt.started_at)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc));
    match started {
        Some(at) => {
            now.signed_duration_since(at)
                < chrono::Duration::from_std(ATTEMPT_STALE_AFTER)
                    .unwrap_or_else(|_| chrono::Duration::minutes(30))
        }
        None => false,
    }
}

/// Can an update start? `unknown` method → no, with the reason (ADR-0045:
/// declared absence, never a guess); an attempt still running → no. Never
/// refused for active Runs.
pub fn apply_gate(
    method: InstallMethod,
    last: Option<&UpdateAttempt>,
    now: chrono::DateTime<chrono::Utc>,
) -> (bool, Option<String>) {
    if method == InstallMethod::Unknown {
        return (
            false,
            Some(
                "Install method not detected (neither a Homebrew Cellar path nor a cargo-dist \
                 receipt): PDO will not guess how to replace its own binary. Update manually, \
                 then restart the daemon."
                    .to_string(),
            ),
        );
    }
    if let Some(a) = last {
        if attempt_is_running(a, now) {
            return (
                false,
                Some(format!(
                    "An update attempt ({}) is already running.",
                    a.attempt_id
                )),
            );
        }
    }
    (true, None)
}

/// An attempt id is filesystem-safe: ASCII letters, digits, `-` and `_`.
pub fn is_valid_attempt_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(status: AttemptStatus, started_at: &str) -> UpdateAttempt {
        UpdateAttempt {
            schema: ATTEMPT_SCHEMA.into(),
            attempt_id: "a1".into(),
            status,
            started_at: started_at.into(),
            finished_at: None,
            exit_code: None,
            method: InstallMethod::Homebrew,
            command: "brew".into(),
            supervision: Supervision::None,
            log_path: PathBuf::from("/tmp/a1.log"),
            from_version: "1.0.0".into(),
        }
    }

    #[test]
    fn an_unknown_method_never_applies() {
        let now = chrono::Utc::now();
        let (ok, reason) = apply_gate(InstallMethod::Unknown, None, now);
        assert!(!ok);
        assert!(reason.unwrap().starts_with("Install method not detected"));
        assert_eq!(apply_gate(InstallMethod::Script, None, now), (true, None));
    }

    #[test]
    fn a_running_attempt_blocks_until_it_goes_stale() {
        let now = chrono::Utc::now();
        let fresh = attempt(AttemptStatus::Running, &now.to_rfc3339());
        let (ok, reason) = apply_gate(InstallMethod::Homebrew, Some(&fresh), now);
        assert!(!ok);
        assert_eq!(
            reason.as_deref(),
            Some("An update attempt (a1) is already running.")
        );
        let stale = attempt(
            AttemptStatus::Running,
            &(now - chrono::Duration::minutes(31)).to_rfc3339(),
        );
        assert!(!attempt_is_running(&stale, now));
        assert!(!attempt_is_running(
            &attempt(AttemptStatus::Running, "garbage"),
            now
        ));
        let done = attempt(AttemptStatus::Failed, &now.to_rfc3339());
        assert_eq!(
            apply_gate(InstallMethod::Homebrew, Some(&done), now),
            (true, None)
        );
    }

    #[test]
    fn finishing_records_the_code_and_keeps_the_scripts_end_date() {
        let mut a = attempt(AttemptStatus::Running, "t0");
        a.finish(0, || "t1".into());
        assert_eq!(a.status, AttemptStatus::Succeeded);
        assert_eq!(a.finished_at.as_deref(), Some("t1"));
        a.finish(3, || "t2".into());
        assert_eq!(a.status, AttemptStatus::Failed);
        assert_eq!(a.exit_code, Some(3));
        assert_eq!(a.finished_at.as_deref(), Some("t1"));
    }

    #[test]
    fn attempt_ids_are_filesystem_safe() {
        assert!(is_valid_attempt_id("20260905-120000-abc123"));
        assert!(!is_valid_attempt_id(""));
        assert!(!is_valid_attempt_id("../etc"));
        assert!(!is_valid_attempt_id("a b"));
    }
}
